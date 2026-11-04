//! The values of sensitive columns in what agents read: a result, the error of a statement and
//! what the user shares of the open tab. They read as the grid shows them while it masks
//! (`••••••`, NULL stays NULL), unless the user chose in the app to show them on the connection.
//!
//! A help against reading them by accident, not a wall: columns are told by their names, in the
//! result and in the select list, so a value that a subquery renames, a second branch of a
//! `UNION` or a `WHERE` that tests it still get through. The database's own permissions are the
//! wall (see docs/AGENTS.md).

use platypus_core::DriverKind;
use platypus_sql::Dialect;
use platypus_sql::sensitive::{
    MASKED_VALUE, NameRules, SelectOutput, has_subquery, select_outputs,
};
use platypus_sql::source_table;
use platypus_store::{AgentValues, SavedConnection, SensitiveColumn};

use crate::agent::Agent;
use crate::error::{AgentError, Result};
use crate::library::SensitiveNames;
use crate::link::{ContextResult, UserContext};
use crate::query::{Cell, Column, Outcome, Table};

/// Which columns of one statement's result are sensitive.
struct ResultRules {
    names: SensitiveNames,
    types: NameRules,
    /// The user's marks on the one table the statement reads, when it reads one: they count
    /// over the rules. Without a table a name is not tied to one, so any mark "sensitive" on it
    /// counts and no mark lifts the rules.
    table: Option<Vec<SensitiveColumn>>,
}

impl ResultRules {
    /// The mark on `name` of the statement's table. Where more than one table may be meant (a
    /// statement that names no schema), "sensitive" wins.
    fn mark(&self, name: &str) -> Option<bool> {
        let marks = self.table.as_ref()?;
        let mut on_it = marks
            .iter()
            .filter(|m| m.key.column.eq_ignore_ascii_case(name))
            .peekable();
        on_it.peek()?;
        Some(on_it.any(|m| m.sensitive))
    }

    fn name_is_sensitive(&self, name: &str) -> bool {
        self.mark(name).unwrap_or_else(|| match self.table {
            Some(_) => self.names.by_rule(name),
            None => self.names.is(name),
        })
    }

    fn is_sensitive(&self, column: &Column) -> bool {
        if let Some(mark) = self.mark(&column.name) {
            return mark;
        }
        // The type without its size: `varchar(20)` is `varchar`.
        let db_type = column.db_type.split('(').next().unwrap_or_default().trim();
        self.name_is_sensitive(&column.name) || self.types.matches(db_type)
    }
}

/// Which `columns` show a sensitive value: by their own name or type, by the item of the select
/// list at their position (`lower(email)`, `email AS e`), or by an alias of such an item when a
/// `*` leaves the positions unknown.
fn sensitive_columns(columns: &[Column], items: &[SelectOutput], rules: &ResultRules) -> Vec<bool> {
    let by_position = items.len() == columns.len() && !items.iter().any(|i| i.star);
    columns
        .iter()
        .enumerate()
        .map(|(position, column)| {
            rules.is_sensitive(column)
                || (by_position && items[position].sensitive)
                || items.iter().any(|i| {
                    i.sensitive
                        && i.alias
                            .as_deref()
                            .is_some_and(|a| a.eq_ignore_ascii_case(&column.name))
                })
        })
        .collect()
}

/// The values of the `masked` columns of `table` written as [`MASKED_VALUE`].
fn mask_table(table: &mut Table, masked: &[bool]) {
    for row in &mut table.rows {
        for (cell, masked) in row.iter_mut().zip(masked) {
            if *masked && *cell != Cell::Null {
                *cell = Cell::Text(MASKED_VALUE.to_owned());
            }
        }
    }
    table.masked = table
        .columns
        .iter()
        .zip(masked)
        .filter(|(_, masked)| **masked)
        .map(|(c, _)| c.name.clone())
        .collect();
}

/// Whether statements on `driver` are SQL, which a select list can be read from.
const fn reads_sql(driver: DriverKind) -> bool {
    !matches!(
        driver,
        DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch
    )
}

/// The marks of `marks` on the table `sql` reads, when it reads one.
fn marks_on_table(
    marks: Vec<SensitiveColumn>,
    sql: &str,
    saved: &SavedConnection,
) -> Option<Vec<SensitiveColumn>> {
    let dialect = Dialect::from(saved.driver);
    // A subquery reads another table, whose marks must count: no one table here.
    if !reads_sql(saved.driver) || has_subquery(sql, dialect) {
        return None;
    }
    let source = source_table(sql, dialect)?;
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    Some(
        marks
            .into_iter()
            .filter(|m| {
                same(&m.key.object, &source.name)
                    && source
                        .schema
                        .as_deref()
                        .is_none_or(|s| same(&m.key.schema, s))
                    && (m.key.database.is_empty() || same(&m.key.database, &saved.database))
            })
            .collect(),
    )
}

/// `result` with the values of the columns `is_sensitive` says masked; their names.
fn mask_shared_result(
    result: &mut ContextResult,
    is_sensitive: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let masked: Vec<bool> = result.columns.iter().map(|c| is_sensitive(c)).collect();
    for row in &mut result.rows {
        for (cell, masked) in row.iter_mut().zip(&masked) {
            if *masked && cell.is_some() {
                *cell = Some(MASKED_VALUE.to_owned());
            }
        }
    }
    result
        .columns
        .iter()
        .zip(&masked)
        .filter(|(_, masked)| **masked)
        .map(|(c, _)| c.clone())
        .collect()
}

impl Agent {
    /// `outcome` of `sql` on `saved` as agents read it. Marks that cannot be read would let
    /// values through: the call fails instead.
    pub(crate) fn mask_outcome(
        &self,
        saved: &SavedConnection,
        sql: &str,
        mut outcome: Outcome,
    ) -> Result<Outcome> {
        let Outcome::Rows(table) = &mut outcome else {
            return Ok(outcome);
        };
        if saved.agent_values == AgentValues::Shown {
            return Ok(outcome);
        }
        let sensitivity = self.sensitivity()?;
        let rules = ResultRules {
            names: sensitivity.names(&[saved.id])?,
            types: sensitivity.types().clone(),
            table: marks_on_table(sensitivity.marks(saved.id)?, sql, saved),
        };
        let items = if reads_sql(saved.driver) {
            select_outputs(sql, Dialect::from(saved.driver), &|name| {
                rules.name_is_sensitive(name)
            })
        } else {
            Vec::new()
        };
        let masked = sensitive_columns(&table.columns, &items, &rules);
        mask_table(table, &masked);
        Ok(outcome)
    }

    /// A statement's error on `saved` as agents read it: the values it quotes of sensitive
    /// columns masked. Other errors stay as they are.
    pub(crate) fn mask_statement_error(
        &self,
        saved: &SavedConnection,
        error: AgentError,
    ) -> AgentError {
        let AgentError::Database(message) = &error else {
            return error;
        };
        if saved.agent_values == AgentValues::Shown {
            return error;
        }
        match self.sensitivity().and_then(|s| s.names(&[saved.id])) {
            Ok(names) => AgentError::Database(names.mask_error(message)),
            Err(e) => e,
        }
    }

    /// What the user shares of the tab open on `saved` (`None`: on no saved connection) as
    /// agents read it; returns the names of the result's columns masked.
    pub(crate) fn mask_context(
        &self,
        context: &mut UserContext,
        saved: Option<&SavedConnection>,
    ) -> Result<Vec<String>> {
        if saved.is_some_and(|s| s.agent_values == AgentValues::Shown) {
            return Ok(Vec::new());
        }
        let ids: Vec<_> = saved.iter().map(|s| s.id).collect();
        let names = self.sensitivity()?.names(&ids)?;
        let dialect = saved.map_or(Dialect::Postgres, |s| Dialect::from(s.driver));
        for sql in [&mut context.editor, &mut context.selection]
            .into_iter()
            .flatten()
        {
            *sql = names.mask(sql, dialect);
        }
        Ok(context
            .result
            .as_mut()
            .map(|result| mask_shared_result(result, &|c| names.is(c)))
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use platypus_store::CatalogKey;

    use super::*;

    fn column(name: &str, db_type: &str) -> Column {
        Column {
            name: name.to_owned(),
            db_type: db_type.to_owned(),
        }
    }

    fn mark(schema: &str, column: &str, sensitive: bool) -> SensitiveColumn {
        SensitiveColumn {
            key: CatalogKey {
                database: String::new(),
                schema: schema.to_owned(),
                object: "users".to_owned(),
                column: column.to_owned(),
            },
            sensitive,
        }
    }

    fn rules(table: Option<Vec<SensitiveColumn>>) -> ResultRules {
        ResultRules {
            names: SensitiveNames::new(
                NameRules::new("*email*"),
                HashSet::from(["private_note".to_owned()]),
            ),
            types: NameRules::new("inet"),
            table,
        }
    }

    #[test]
    fn a_statement_on_no_one_table_goes_by_the_rules_and_any_mark() {
        let r = rules(None);
        assert!(r.is_sensitive(&column("Email", "text")));
        assert!(r.is_sensitive(&column("private_note", "text")));
        assert!(r.is_sensitive(&column("ip", "inet(4)")));
        assert!(!r.is_sensitive(&column("name", "text")));
    }

    #[test]
    fn the_marks_of_the_table_count_over_the_rules() {
        let r = rules(Some(vec![
            mark("main", "email", false),
            mark("main", "nick", true),
        ]));
        assert!(
            !r.is_sensitive(&column("email", "text")),
            "marked not sensitive"
        );
        assert!(r.is_sensitive(&column("NICK", "text")));
        assert!(
            r.is_sensitive(&column("backup_email", "text")),
            "by its rule"
        );
        // Marked on another table: not this one's.
        assert!(!r.is_sensitive(&column("private_note", "text")));
    }

    #[test]
    fn sensitive_wins_when_several_tables_may_be_meant() {
        let r = rules(Some(vec![
            mark("a", "email", false),
            mark("b", "email", true),
        ]));
        assert!(r.is_sensitive(&column("email", "text")));
    }

    #[test]
    fn items_of_the_select_list_mask_by_position_or_by_alias() {
        let r = rules(None);
        let items = |sql: &str| select_outputs(sql, Dialect::Postgres, &|n| r.name_is_sensitive(n));
        let columns = [column("id", "int"), column("lower", "text")];
        assert_eq!(
            sensitive_columns(&columns, &items("SELECT id, lower(email) FROM u"), &r),
            [false, true]
        );
        // A `*` leaves the positions unknown: an alias still tells.
        let columns = [column("id", "int"), column("e", "text")];
        assert_eq!(
            sensitive_columns(&columns, &items("SELECT *, email AS e FROM u"), &r),
            [false, true]
        );
        // No select list to read: the names alone.
        assert_eq!(sensitive_columns(&columns, &[], &r), [false, false]);
    }

    #[test]
    fn masked_values_read_the_same_and_null_stays_null() {
        let mut table = Table {
            columns: vec![column("id", "int"), column("email", "text")],
            rows: vec![
                vec![Cell::Number("1".to_owned()), Cell::Text("a@b.c".to_owned())],
                vec![Cell::Number("2".to_owned()), Cell::Null],
                vec![Cell::Number("3".to_owned()), Cell::Number("9".to_owned())],
            ],
            cut: None,
            values_cut: false,
            masked: Vec::new(),
        };
        mask_table(&mut table, &[false, true]);
        assert_eq!(table.rows[0][1], Cell::Text(MASKED_VALUE.to_owned()));
        assert_eq!(table.rows[1][1], Cell::Null);
        assert_eq!(table.rows[2][1], Cell::Text(MASKED_VALUE.to_owned()));
        assert_eq!(table.rows[0][0], Cell::Number("1".to_owned()));
        assert_eq!(table.masked, ["email"]);
    }

    #[test]
    fn a_shared_result_masks_by_its_column_names() {
        let mut result = ContextResult {
            columns: vec!["id".to_owned(), "email".to_owned()],
            rows: vec![
                vec![Some("1".to_owned()), Some("a@b.c".to_owned())],
                vec![None, None],
            ],
            truncated: false,
            total_rows: None,
        };
        let names = mask_shared_result(&mut result, &|c| c == "email");
        assert_eq!(names, ["email"]);
        assert_eq!(
            result.rows[0],
            [Some("1".to_owned()), Some(MASKED_VALUE.to_owned())]
        );
        assert_eq!(result.rows[1], [None, None]);
    }
}
