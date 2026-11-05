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
use serde_json::Value;

use crate::agent::Agent;
use crate::error::{AgentError, Result};
use crate::library::SensitiveNames;
use crate::link::{ContextResult, ContextTable, UserContext};
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
        self.is_sensitive_column(&column.name, &column.db_type)
    }

    fn is_sensitive_column(&self, name: &str, db_type: &str) -> bool {
        if let Some(mark) = self.mark(name) {
            return mark;
        }
        // The type without its size: `varchar(20)` is `varchar`.
        let db_type = db_type.split('(').next().unwrap_or_default().trim();
        self.name_is_sensitive(name) || self.types.matches(db_type)
    }
}

impl ResultRules {
    /// Whether the field `leaf` at `path` (`user.password`: the column, then the fields down to
    /// it) of a document is sensitive: by a mark on the path, else by the rules on its own name.
    /// Without one table, a mark on the path or on the name of any table counts.
    fn path_is_sensitive(&self, path: &str, leaf: &str) -> bool {
        self.mark(path).unwrap_or_else(|| match self.table {
            Some(_) => self.names.by_rule(leaf),
            None => self.names.is(path) || self.names.is(leaf),
        })
    }

    /// The document or array written in `text`, the value of the column `column`, with the
    /// value of each sensitive field at any depth written as [`MASKED_VALUE`] (`null` stays);
    /// `None` when there is none to mask. The paths masked are added to `found`.
    fn mask_document(&self, column: &str, text: &str, found: &mut Vec<String>) -> Option<String> {
        if !text.trim_start().starts_with(['{', '[']) {
            return None;
        }
        let mut value: Value = serde_json::from_str(text).ok()?;
        let before = found.len();
        self.mask_fields(&mut value, column, found);
        (found.len() > before).then(|| value.to_string())
    }

    fn mask_fields(&self, value: &mut Value, path: &str, found: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, field) in map.iter_mut() {
                    let at = format!("{path}.{key}");
                    if !self.path_is_sensitive(&at, key) {
                        self.mask_fields(field, &at, found);
                    } else if !field.is_null() {
                        *field = Value::String(MASKED_VALUE.to_owned());
                        found.push(at);
                    }
                }
            }
            // The items of an array share the array's path.
            Value::Array(items) => items
                .iter_mut()
                .for_each(|item| self.mask_fields(item, path, found)),
            _ => {}
        }
    }
}

/// Whether a column of this `db_type` (MongoDB's name for the type) may hold fields.
fn holds_fields(db_type: &str) -> bool {
    ["document", "array", "mixed"]
        .iter()
        .any(|t| db_type.eq_ignore_ascii_case(t))
}

/// Which columns of one known table are sensitive for agents: the marks on it count over the
/// rules. Without rules (`None`) the connection shows the values.
pub(crate) struct TableSensitivity(Option<ResultRules>);

impl TableSensitivity {
    /// Whether the column `name` of type `db_type` must show no value.
    pub(crate) fn is_sensitive(&self, name: &str, db_type: &str) -> bool {
        self.0
            .as_ref()
            .is_some_and(|rules| rules.is_sensitive_column(name, db_type))
    }

    /// The document or array in `text`, the value of the column `column`, with its sensitive
    /// fields masked; `None` when nothing in it is.
    pub(crate) fn mask_document(&self, column: &str, text: &str) -> Option<String> {
        self.0
            .as_ref()?
            .mask_document(column, text, &mut Vec::new())
    }

    /// Rules that match `email` by name and the given `marks`, for tests.
    #[cfg(test)]
    pub(crate) fn for_test(marks: Vec<SensitiveColumn>) -> Self {
        Self(Some(ResultRules {
            names: SensitiveNames::new(
                NameRules::new("*email*"),
                std::collections::HashSet::default(),
            ),
            types: NameRules::default(),
            table: Some(marks),
        }))
    }

    /// Whether the column of type `db_type` may hold fields that need masking.
    pub(crate) fn may_hold_fields(&self, db_type: &str) -> bool {
        self.0.is_some() && holds_fields(db_type)
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
    let rest = table.rest.iter_mut().flat_map(|r| r.rows.iter_mut());
    for row in table.rows.iter_mut().chain(rest) {
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
    Some(marks_of_table(
        marks,
        source.schema.as_deref(),
        &source.name,
        saved,
    ))
}

/// The marks of `marks` on the table `name` (of `schema`, when known) of `saved`.
fn marks_of_table(
    marks: Vec<SensitiveColumn>,
    schema: Option<&str>,
    name: &str,
    saved: &SavedConnection,
) -> Vec<SensitiveColumn> {
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    marks
        .into_iter()
        .filter(|m| {
            same(&m.key.object, name)
                && schema.is_none_or(|s| same(&m.key.schema, s))
                && (m.key.database.is_empty() || same(&m.key.database, &saved.database))
        })
        .collect()
}

/// `result` with the values of the `masked` columns written as [`MASKED_VALUE`]; their names.
fn mask_shared_result(result: &mut ContextResult, masked: &[bool]) -> Vec<String> {
    for row in &mut result.rows {
        for (cell, masked) in row.iter_mut().zip(masked) {
            if *masked && cell.is_some() {
                *cell = Some(MASKED_VALUE.to_owned());
            }
        }
    }
    result
        .columns
        .iter()
        .zip(masked)
        .filter(|(_, masked)| **masked)
        .map(|(c, _)| c.clone())
        .collect()
}

/// Masks the fields of the documents in the columns of `table` that hold them (but those masked
/// whole), and adds the paths masked to its `masked` list.
fn mask_documents(table: &mut Table, masked: &[bool], rules: &ResultRules) {
    let holds: Vec<bool> = table
        .columns
        .iter()
        .zip(masked)
        .map(|(c, masked)| !masked && holds_fields(&c.db_type))
        .collect();
    if !holds.contains(&true) {
        return;
    }
    let mut found = Vec::new();
    let rest = table.rest.iter_mut().flat_map(|r| r.rows.iter_mut());
    for row in table.rows.iter_mut().chain(rest) {
        for ((cell, column), holds) in row.iter_mut().zip(&table.columns).zip(&holds) {
            if let (true, Cell::Text(text)) = (*holds, &*cell)
                && let Some(masked) = rules.mask_document(&column.name, text, &mut found)
            {
                *cell = Cell::Text(masked);
            }
        }
    }
    found.sort();
    found.dedup();
    table.masked.extend(found);
}

impl Agent {
    /// Which columns of `schema.table` on `saved` are sensitive, with the marks on that table
    /// counting over the rules. Marks that cannot be read would let values through: the call
    /// fails instead.
    pub(crate) fn table_sensitivity(
        &self,
        saved: &SavedConnection,
        schema: &str,
        table: &str,
    ) -> Result<TableSensitivity> {
        if saved.agent_values == AgentValues::Shown {
            return Ok(TableSensitivity(None));
        }
        let sensitivity = self.sensitivity()?;
        Ok(TableSensitivity(Some(ResultRules {
            names: sensitivity.names(&[saved.id])?,
            types: sensitivity.types().clone(),
            table: Some(marks_of_table(
                sensitivity.marks(saved.id)?,
                Some(schema),
                table,
                saved,
            )),
        })))
    }

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
        if saved.driver == DriverKind::Mongo {
            mask_documents(table, &masked, &rules);
        }
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
        let sensitivity = self.sensitivity()?;
        let ids: Vec<_> = saved.iter().map(|s| s.id).collect();
        let names = sensitivity.names(&ids)?;
        // No rule and no mark: nothing is sensitive.
        if names.is_empty() && sensitivity.types().is_empty() {
            return Ok(Vec::new());
        }
        let marks = match saved {
            Some(s) if context.result.is_some() => sensitivity.marks(s.id)?,
            _ => Vec::new(),
        };
        let rules = ResultRules {
            names,
            types: sensitivity.types().clone(),
            table: None,
        };
        Ok(mask_shared(context, rules, marks, saved))
    }
}

/// What the user shares, with the statements' literals and the result's values masked; the names
/// of the result's masked columns. The result's columns are told as `run_query` tells them,
/// through the statements it may have come from (the selection and the editor's text: which ran
/// is not known, so a column masked by either is) or the table of a viewer tab.
fn mask_shared(
    context: &mut UserContext,
    mut rules: ResultRules,
    marks: Vec<SensitiveColumn>,
    saved: Option<&SavedConnection>,
) -> Vec<String> {
    let dialect = saved.map_or(Dialect::Postgres, |s| Dialect::from(s.driver));
    // The statements as the user wrote them: the result is read through them below.
    let statements: Vec<String> = [&context.selection, &context.editor]
        .into_iter()
        .flatten()
        .filter(|sql| !sql.trim().is_empty())
        .cloned()
        .collect();
    for sql in [&mut context.editor, &mut context.selection]
        .into_iter()
        .flatten()
    {
        *sql = rules.names.mask(sql, dialect);
    }
    let scopes = shared_scopes(&statements, context.table.as_ref(), marks, saved);
    let Some(result) = context.result.as_mut() else {
        return Vec::new();
    };
    let columns: Vec<Column> = result
        .columns
        .iter()
        .map(|name| Column {
            name: name.clone(),
            db_type: String::new(),
        })
        .collect();
    let mut masked = vec![false; columns.len()];
    for (statement, table) in scopes {
        rules.table = table;
        let items = statement_items(statement.as_deref(), saved, &rules);
        if cannot_follow(statement.as_deref(), &items, saved, &rules) {
            masked.fill(true);
            break;
        }
        let by_scope = sensitive_columns(&columns, &items, &rules);
        masked.iter_mut().zip(by_scope).for_each(|(m, s)| *m |= s);
    }
    let mut names = mask_shared_result(result, &masked);
    if saved.is_some_and(|s| s.driver == DriverKind::Mongo) {
        // Which collection the documents are of is not known: any mark counts.
        rules.table = None;
        names.extend(mask_shared_documents(result, &masked, &rules));
    }
    names
}

/// What a shared result may have come from, each with the marks of the one table it reads (none
/// when it reads not exactly one): the statements, and the table of a viewer tab. Without any,
/// a scope that knows nothing.
fn shared_scopes(
    statements: &[String],
    viewer: Option<&ContextTable>,
    marks: Vec<SensitiveColumn>,
    saved: Option<&SavedConnection>,
) -> Vec<(Option<String>, Option<Vec<SensitiveColumn>>)> {
    let mut scopes: Vec<_> = statements
        .iter()
        .map(|sql| {
            let table = saved.and_then(|s| marks_on_table(marks.clone(), sql, s));
            (Some(sql.clone()), table)
        })
        .collect();
    if let (Some(viewer), Some(saved)) = (viewer, saved) {
        let table = marks_of_table(marks, Some(&viewer.schema), &viewer.name, saved);
        scopes.push((None, Some(table)));
    }
    if scopes.is_empty() {
        scopes.push((None, None));
    }
    scopes
}

/// The select list of `statement` when it is SQL that can be read, else none.
fn statement_items(
    statement: Option<&str>,
    saved: Option<&SavedConnection>,
    rules: &ResultRules,
) -> Vec<SelectOutput> {
    match statement {
        Some(sql) if saved.is_none_or(|s| reads_sql(s.driver)) => select_outputs(
            sql,
            saved.map_or(Dialect::Postgres, |s| Dialect::from(s.driver)),
            &|name| rules.name_is_sensitive(name),
        ),
        _ => Vec::new(),
    }
}

/// Whether the columns of the result of SQL `statement` cannot be traced to the columns they read,
/// while it names a sensitive one: it has a subquery (which can rename it) or no select list
/// could be read (it does not parse). The shared result is then masked whole, where `run_query`,
/// which has the real names of the columns, goes by them.
fn cannot_follow(
    statement: Option<&str>,
    items: &[SelectOutput],
    saved: Option<&SavedConnection>,
    rules: &ResultRules,
) -> bool {
    let Some(sql) = statement else {
        return false;
    };
    let dialect = saved.map_or(Dialect::Postgres, |s| Dialect::from(s.driver));
    if !saved.is_none_or(|s| reads_sql(s.driver))
        || !(items.is_empty() || has_subquery(sql, dialect))
    {
        return false;
    }
    sql.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| !word.is_empty() && rules.names.is(word))
}

/// Masks the fields in the cells of the shared `result` that are no masked column; their paths.
fn mask_shared_documents(
    result: &mut ContextResult,
    masked: &[bool],
    rules: &ResultRules,
) -> Vec<String> {
    let mut found = Vec::new();
    for row in &mut result.rows {
        for ((cell, column), masked) in row.iter_mut().zip(&result.columns).zip(masked) {
            if let (false, Some(text)) = (*masked, &*cell)
                && let Some(text) = rules.mask_document(column, text, &mut found)
            {
                *cell = Some(text);
            }
        }
    }
    found.sort();
    found.dedup();
    found
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
            rest: None,
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
        let names = mask_shared_result(&mut result, &[false, true]);
        assert_eq!(names, ["email"]);
        assert_eq!(
            result.rows[0],
            [Some("1".to_owned()), Some(MASKED_VALUE.to_owned())]
        );
        assert_eq!(result.rows[1], [None, None]);
    }

    fn saved(driver: DriverKind) -> SavedConnection {
        SavedConnection::new("t", driver)
    }

    /// The columns of a shared result `columns` (one row of `secret` in each) that `editor`
    /// leaves shown, on a SQLite connection, with the user's `marks`.
    fn shown_in_context(
        editor: Option<&str>,
        viewer: Option<(&str, &str)>,
        columns: &[&str],
        marks: Vec<SensitiveColumn>,
    ) -> Vec<bool> {
        let mut context = UserContext {
            editor: editor.map(str::to_owned),
            table: viewer.map(|(schema, name)| ContextTable {
                schema: schema.to_owned(),
                name: name.to_owned(),
            }),
            result: Some(ContextResult {
                columns: columns.iter().map(|c| (*c).to_owned()).collect(),
                rows: vec![vec![Some("secret".to_owned()); columns.len()]],
                truncated: false,
                total_rows: None,
            }),
            ..UserContext::default()
        };
        // The names the user marked sensitive are known to the rules, as in the app.
        let mut rules = rules(None);
        rules.names = SensitiveNames::new(
            NameRules::new("*email*"),
            marks
                .iter()
                .filter(|m| m.sensitive)
                .map(|m| m.key.column.to_lowercase())
                .collect(),
        );
        mask_shared(&mut context, rules, marks, Some(&saved(DriverKind::Sqlite)));
        context.result.unwrap().rows[0]
            .iter()
            .map(|c| c.as_deref() == Some("secret"))
            .collect()
    }

    #[test]
    fn an_alias_of_a_sensitive_column_is_masked_in_the_context() {
        let shown = |sql, columns: &[&str]| shown_in_context(Some(sql), None, columns, Vec::new());
        assert_eq!(
            shown("SELECT email AS p, id FROM users", &["p", "id"]),
            [false, true]
        );
        // The same statement with a column that is no one's secret.
        assert_eq!(shown("SELECT name AS p FROM users", &["p"]), [true]);
        // `SELECT *` leaves the names: they tell.
        assert_eq!(
            shown("SELECT * FROM users", &["id", "email"]),
            [true, false]
        );
        assert_eq!(
            shown("SELECT *, email AS e FROM users", &["id", "e"]),
            [true, false]
        );
        // An expression over a sensitive column.
        assert_eq!(
            shown("SELECT lower(email), id FROM users", &["lower", "id"]),
            [false, true]
        );
    }

    #[test]
    fn a_statement_that_cannot_be_followed_masks_the_whole_result() {
        let shown = |sql, columns: &[&str]| shown_in_context(Some(sql), None, columns, Vec::new());
        // A subquery renames the column: nothing says which one it is.
        assert_eq!(
            shown(
                "SELECT n, id FROM (SELECT email AS n, id FROM users) t",
                &["n", "id"]
            ),
            [false, false]
        );
        // One that does not parse.
        assert_eq!(
            shown("SELEC email AS p, id FRM users", &["p", "id"]),
            [false, false]
        );
        // They name no sensitive column: nothing to hide.
        assert_eq!(
            shown("SELECT n FROM (SELECT name AS n FROM users) t", &["n"]),
            [true]
        );
    }

    #[test]
    fn marks_decide_for_the_table_the_statement_reads() {
        // `email` is marked not sensitive on `users`: the statement reads it, and shows it.
        let marks = vec![mark("main", "email", false)];
        assert_eq!(
            shown_in_context(
                Some("SELECT email FROM users"),
                None,
                &["email"],
                marks.clone()
            ),
            [true]
        );
        // A statement that reads two tables does not say which one a `note` is of: the mark
        // "sensitive" on either counts (this over-masks the other table's column).
        let marks = vec![mark("main", "note", true)];
        assert_eq!(
            shown_in_context(
                Some("SELECT a.note, b.note FROM users a JOIN orders b ON a.id = b.id"),
                None,
                &["note", "note"],
                marks.clone()
            ),
            [false, false]
        );
        // A viewer tab reads the table it shows: its marks count over the rules.
        let marks = vec![mark("main", "email", false), mark("main", "nick", true)];
        assert_eq!(
            shown_in_context(
                None,
                Some(("main", "users")),
                &["email", "nick"],
                marks.clone()
            ),
            [true, false]
        );
        // Another table's viewer: the rules alone.
        assert_eq!(
            shown_in_context(None, Some(("main", "orders")), &["email", "nick"], marks),
            [false, true]
        );
    }

    #[test]
    fn nothing_is_known_of_a_tab_without_a_statement_but_the_names() {
        assert_eq!(
            shown_in_context(None, None, &["email", "id"], Vec::new()),
            [false, true]
        );
    }

    #[test]
    fn nested_fields_are_masked_by_path_and_by_name() {
        let r = rules(Some(vec![mark("main", "user.pin", true)]));
        let mut found = Vec::new();
        let doc = r#"{"name":"Ann","email":"a@b.c","pin":"1234","tag":null,"contact":{"email":"x","phones":[{"email":"q","n":1}]}}"#;
        let masked = r.mask_document("user", doc, &mut found).unwrap();
        let value: Value = serde_json::from_str(&masked).unwrap();
        assert_eq!(value["name"], "Ann");
        assert_eq!(value["email"], MASKED_VALUE);
        assert_eq!(value["pin"], MASKED_VALUE, "marked by its path");
        assert_eq!(value["contact"]["email"], MASKED_VALUE);
        assert_eq!(value["contact"]["phones"][0]["email"], MASKED_VALUE);
        assert_eq!(value["contact"]["phones"][0]["n"], 1);
        assert!(!masked.contains("a@b.c") && !masked.contains("1234") && !masked.contains('q'));
        assert!(found.contains(&"user.pin".to_owned()), "{found:?}");
        // A mark on another path is not this field's.
        assert!(
            r.mask_document("other", r#"{"pin":"1"}"#, &mut Vec::new())
                .is_none()
        );
        // An array of documents, null kept, and text that is no document.
        let list = r.mask_document("l", r#"[{"email":null},{"email":"z"}]"#, &mut Vec::new());
        assert_eq!(
            list.unwrap(),
            format!(r#"[{{"email":null}},{{"email":"{MASKED_VALUE}"}}]"#)
        );
        assert!(r.mask_document("c", "hello", &mut Vec::new()).is_none());
        assert!(
            r.mask_document("c", r#"{"a":1}"#, &mut Vec::new())
                .is_none()
        );
    }

    #[test]
    fn a_document_column_masks_its_fields_in_results() {
        let mut table = Table {
            columns: vec![
                column("_id", "objectId"),
                Column {
                    name: "user".to_owned(),
                    db_type: "document".to_owned(),
                },
            ],
            rows: vec![vec![
                Cell::Text("1".to_owned()),
                Cell::Text(r#"{"name":"Ann","email":"a@b.c"}"#.to_owned()),
            ]],
            cut: None,
            values_cut: false,
            masked: Vec::new(),
            rest: None,
        };
        mask_documents(&mut table, &[false, false], &rules(None));
        let Cell::Text(text) = &table.rows[0][1] else {
            panic!("text");
        };
        assert!(text.contains("Ann") && !text.contains("a@b.c"), "{text}");
        assert_eq!(table.masked, ["user.email"]);
    }

    #[test]
    fn a_context_of_documents_masks_nested_fields() {
        let mut context = UserContext {
            result: Some(ContextResult {
                columns: vec!["user".to_owned()],
                rows: vec![vec![Some(r#"{"email":"a@b.c","n":1}"#.to_owned())]],
                truncated: false,
                total_rows: None,
            }),
            ..UserContext::default()
        };
        let names = mask_shared(
            &mut context,
            rules(None),
            Vec::new(),
            Some(&saved(DriverKind::Mongo)),
        );
        assert_eq!(names, ["user.email"]);
        let cell = context.result.unwrap().rows[0][0].clone().unwrap();
        assert!(
            !cell.contains("a@b.c") && cell.contains("\"n\":1"),
            "{cell}"
        );
    }
}
