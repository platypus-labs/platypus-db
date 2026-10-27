//! Plans from `EXPLAIN PLAN`: the steps read from the plan table, the text from
//! `DBMS_XPLAN`. The plan table is the session's own (a temporary table): the rows of a
//! plan are deleted once read.

use std::sync::atomic::{AtomicU64, Ordering};

use platypus_core::{
    Connection, ExecOptions, Explain, ExplainFormat, PlanNode, Result, TextRow, TextRowExt,
    query_text,
};

/// Numbers the plans of the process, which the plan table tells apart by statement id.
static NEXT: AtomicU64 = AtomicU64::new(1);

/// The plan of `sql`, without running it.
pub async fn explain(conn: &dyn Connection, sql: &str) -> Result<Explain> {
    let id = format!("platypus_{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let options = ExecOptions::default();
    conn.execute(
        &format!("EXPLAIN PLAN SET STATEMENT_ID = '{id}' FOR {sql}"),
        &options,
    )
    .await?;
    let read = async {
        let text = query_text(
            conn,
            &format!(
                "SELECT PLAN_TABLE_OUTPUT FROM \
                 TABLE(DBMS_XPLAN.DISPLAY('PLAN_TABLE', '{id}', 'TYPICAL'))"
            ),
        )
        .await?;
        let steps = query_text(
            conn,
            &format!(
                "SELECT ID, PARENT_ID, OPERATION, OPTIONS, OBJECT_OWNER, OBJECT_NAME, COST, \
                 CARDINALITY, BYTES, ACCESS_PREDICATES, FILTER_PREDICATES FROM PLAN_TABLE \
                 WHERE STATEMENT_ID = '{id}' ORDER BY ID"
            ),
        )
        .await?;
        Ok::<_, platypus_core::Error>((text, steps))
    }
    .await;
    let _ = conn
        .execute(
            &format!("DELETE FROM PLAN_TABLE WHERE STATEMENT_ID = '{id}'"),
            &options,
        )
        .await;
    let (text, steps) = read?;
    let plan: Vec<String> = text.iter().map(|r| r.text(0)).collect();
    Ok(Explain::new(
        ExplainFormat::Text,
        plan.join("\n"),
        plan_tree(&steps),
        false,
    ))
}

/// One step of the plan table's rows.
fn step(row: &TextRow) -> PlanNode {
    let mut node = PlanNode::new(format!("{} {}", row.text(2), row.text(3)).trim().to_owned());
    node.detail = row.opt(5).map(|name| match row.opt(4) {
        Some(owner) => format!("{owner}.{name}"),
        None => name,
    });
    node.cost = row.opt(6).and_then(|c| c.parse().ok());
    node.rows = row.opt(7).and_then(|c| c.parse().ok());
    for (label, column) in [("Bytes", 8), ("Access", 9), ("Filter", 10)] {
        if let Some(value) = row.opt(column) {
            node.properties.push((label.to_owned(), value));
        }
    }
    node
}

/// The tree of the plan table's rows (`ID`, `PARENT_ID`…), children in their order.
fn plan_tree(rows: &[TextRow]) -> PlanNode {
    fn build(rows: &[TextRow], at: usize) -> PlanNode {
        let id = rows[at].text(0);
        let mut node = step(&rows[at]);
        node.children = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.opt(1).as_deref() == Some(id.as_str()))
            .map(|(i, _)| build(rows, i))
            .collect();
        node
    }
    rows.iter()
        .position(|r| r.opt(1).is_none())
        .map_or_else(|| PlanNode::new("Plan"), |root| build(rows, root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[Option<&str>]) -> TextRow {
        cells.iter().map(|c| c.map(str::to_owned)).collect()
    }

    #[test]
    fn builds_the_plan_tree() {
        let rows = [
            row(&[
                Some("0"),
                None,
                Some("SELECT STATEMENT"),
                None,
                None,
                None,
                Some("2"),
                Some("1"),
            ]),
            row(&[
                Some("1"),
                Some("0"),
                Some("TABLE ACCESS"),
                Some("BY INDEX ROWID"),
                Some("HR"),
                Some("EMP"),
                Some("2"),
                Some("1"),
            ]),
            row(&[
                Some("2"),
                Some("1"),
                Some("INDEX"),
                Some("UNIQUE SCAN"),
                Some("HR"),
                Some("EMP_PK"),
                Some("1"),
                Some("1"),
                None,
                Some("\"ID\"=1"),
            ]),
        ];
        let root = plan_tree(&rows);
        assert_eq!(root.operation, "SELECT STATEMENT");
        let access = &root.children[0];
        assert_eq!(access.operation, "TABLE ACCESS BY INDEX ROWID");
        assert_eq!(access.detail.as_deref(), Some("HR.EMP"));
        let index = &access.children[0];
        assert_eq!(index.operation, "INDEX UNIQUE SCAN");
        assert_eq!(
            index.properties,
            [("Access".to_owned(), "\"ID\"=1".to_owned())]
        );
        assert_eq!(index.cost, Some(1.0));
    }
}
