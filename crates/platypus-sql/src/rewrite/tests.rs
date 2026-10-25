use super::*;

const PG: Dialect = Dialect::Postgres;
const MY: Dialect = Dialect::MySql;
const MS: Dialect = Dialect::SqlServer;

fn sort(sql: &str, dialect: Dialect, column: &str, desc: bool) -> Option<String> {
    set_order_by(sql, dialect, Some((column, desc)))
}

fn sorted(sql: &str, dialect: Dialect) -> String {
    sort(sql, dialect, "a", false).unwrap()
}

fn unsorted(sql: &str, dialect: Dialect) -> String {
    set_order_by(sql, dialect, None).unwrap()
}

fn count(sql: &str, dialect: Dialect) -> String {
    count_query(sql, dialect).unwrap()
}

fn filtered(sql: &str, dialect: Dialect, condition: &str) -> String {
    add_condition(sql, dialect, condition).unwrap()
}

#[test]
fn adds_to_a_plain_query() {
    assert_eq!(
        sort(
            "SELECT * FROM form WHERE sendable_email = true",
            PG,
            "id",
            false
        )
        .unwrap(),
        "SELECT * FROM form WHERE sendable_email = true\nORDER BY id ASC"
    );
}

#[test]
fn replaces_an_existing_order_by() {
    assert_eq!(
        sort(
            "select * from t order by a, b desc nulls last limit 5",
            PG,
            "c",
            true
        )
        .unwrap(),
        "select * from t ORDER BY c DESC limit 5"
    );
    assert_eq!(
        sort("SELECT * FROM t\nORDER BY a", PG, "c", false).unwrap(),
        "SELECT * FROM t\nORDER BY c ASC"
    );
}

#[test]
fn removing_the_sort_removes_the_clause() {
    assert_eq!(
        unsorted("SELECT * FROM t\nORDER BY a DESC", PG),
        "SELECT * FROM t"
    );
    assert_eq!(
        unsorted("SELECT * FROM t ORDER BY a LIMIT 3", PG),
        "SELECT * FROM t LIMIT 3"
    );
    assert_eq!(unsorted("SELECT * FROM t", PG), "SELECT * FROM t");
    assert_eq!(
        unsorted("SELECT * FROM t -- note\nORDER BY a", PG),
        "SELECT * FROM t -- note\n"
    );
}

#[test]
fn removing_the_sort_keeps_what_offset_needs_on_transact_sql() {
    assert_eq!(
        unsorted(
            "SELECT * FROM t ORDER BY a OFFSET 1 ROWS FETCH NEXT 2 ROWS ONLY",
            MS
        ),
        "SELECT * FROM t ORDER BY (SELECT NULL) OFFSET 1 ROWS FETCH NEXT 2 ROWS ONLY"
    );
    // Without OFFSET the clause goes, and elsewhere too.
    assert_eq!(
        unsorted("SELECT TOP (3) * FROM t ORDER BY a", MS),
        "SELECT TOP (3) * FROM t"
    );
    assert_eq!(
        unsorted("SELECT * FROM t ORDER BY a OFFSET 1 ROWS", PG),
        "SELECT * FROM t OFFSET 1 ROWS"
    );
}

#[test]
fn goes_before_limit_offset_and_fetch() {
    assert_eq!(
        sorted("SELECT * FROM t LIMIT 10 OFFSET 5", PG),
        "SELECT * FROM t\nORDER BY a ASC LIMIT 10 OFFSET 5"
    );
    assert_eq!(
        sorted("SELECT * FROM t OFFSET 5", MY),
        "SELECT * FROM t\nORDER BY a ASC OFFSET 5"
    );
    assert_eq!(
        sort(
            "SELECT * FROM t OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY",
            MS,
            "a",
            true
        )
        .unwrap(),
        "SELECT * FROM t\nORDER BY a DESC OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY"
    );
    assert_eq!(
        sort(
            "SELECT * FROM t ORDER BY x OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY",
            MS,
            "a",
            true
        )
        .unwrap(),
        "SELECT * FROM t ORDER BY a DESC OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY"
    );
}

#[test]
fn keeps_top_in_transact_sql() {
    assert_eq!(
        sorted("SELECT TOP (10) * FROM t", MS),
        "SELECT TOP (10) * FROM t\nORDER BY a ASC"
    );
}

#[test]
fn goes_before_lock_clauses_and_hints() {
    assert_eq!(
        sorted("SELECT * FROM t FOR UPDATE", PG),
        "SELECT * FROM t\nORDER BY a ASC FOR UPDATE"
    );
    assert_eq!(
        sorted("SELECT * FROM t ORDER BY x FOR SHARE", PG),
        "SELECT * FROM t ORDER BY a ASC FOR SHARE"
    );
    assert_eq!(
        sorted("SELECT * FROM t LOCK IN SHARE MODE", MY),
        "SELECT * FROM t\nORDER BY a ASC LOCK IN SHARE MODE"
    );
    assert_eq!(
        sorted("SELECT * FROM t ORDER BY x OPTION (RECOMPILE)", MS),
        "SELECT * FROM t ORDER BY a ASC OPTION (RECOMPILE)"
    );
    assert_eq!(
        sorted("SELECT * FROM t OPTION (MAXDOP 1)", MS),
        "SELECT * FROM t\nORDER BY a ASC OPTION (MAXDOP 1)"
    );
}

#[test]
fn gives_up_on_what_it_cannot_place() {
    let cases = [
        ("SELECT * FROM t INTO OUTFILE '/tmp/x'", MY),
        ("SELECT * INTO copy FROM t", PG),
        ("SELECT * FROM t FOR SYSTEM_TIME AS OF '2020-01-01'", MS),
        ("SELECT * FROM t FOR XML AUTO", MS),
        (
            "SELECT * FROM emp CONNECT BY PRIOR id = parent ORDER SIBLINGS BY name",
            Dialect::Oracle,
        ),
        (
            "SELECT * FROM t SETTINGS max_threads = 1",
            Dialect::ClickHouse,
        ),
        ("SELECT * FROM t FORMAT JSON", Dialect::ClickHouse),
        ("SELECT * FROM t; SELECT * FROM u", PG),
        ("SELECT * FROM t;\nSELECT 2 ORDER BY 1;", PG),
        ("UPDATE t SET a = 1 RETURNING *", PG),
        ("WITH c AS (SELECT 1) DELETE FROM t", PG),
        ("(SELECT 1) UNION (SELECT 2)", PG),
        ("SELECT * FROM (SELECT 1", PG),
        ("", PG),
    ];
    for (sql, dialect) in cases {
        assert_eq!(sort(sql, dialect, "a", false), None, "{sql}");
        assert_eq!(set_order_by(sql, dialect, None), None, "{sql}");
        assert_eq!(count_query(sql, dialect), None, "{sql}");
        assert_eq!(add_condition(sql, dialect, "a = 1"), None, "{sql}");
    }
}

#[test]
fn leaves_subqueries_ctes_and_windows_alone() {
    let cte = "WITH c AS (SELECT * FROM t ORDER BY a) SELECT * FROM c";
    assert_eq!(
        sort(cte, PG, "b", false).unwrap(),
        format!("{cte}\nORDER BY b ASC")
    );
    let sub = "SELECT * FROM (SELECT * FROM t ORDER BY a LIMIT 3) s";
    assert_eq!(
        sort(sub, PG, "b", true).unwrap(),
        format!("{sub}\nORDER BY b DESC")
    );
    let window = "SELECT row_number() OVER (ORDER BY a) FROM t";
    assert_eq!(unsorted(window, PG), window);
    assert_eq!(sorted(window, PG), format!("{window}\nORDER BY a ASC"));
    assert_eq!(
        sorted("SELECT * FROM (SELECT 1 LIMIT 1) s LIMIT 2", PG),
        "SELECT * FROM (SELECT 1 LIMIT 1) s\nORDER BY a ASC LIMIT 2"
    );
}

#[test]
fn sorts_the_whole_of_a_set_operation() {
    assert_eq!(
        sorted("SELECT a FROM x UNION SELECT a FROM y", PG),
        "SELECT a FROM x UNION SELECT a FROM y\nORDER BY a ASC"
    );
    assert_eq!(
        sort(
            "SELECT a FROM x UNION ALL SELECT a FROM y ORDER BY 1",
            PG,
            "a",
            true
        )
        .unwrap(),
        "SELECT a FROM x UNION ALL SELECT a FROM y ORDER BY a DESC"
    );
}

#[test]
fn ignores_strings_and_comments() {
    assert_eq!(
        sorted("SELECT 'order by x limit 1' FROM t", PG),
        "SELECT 'order by x limit 1' FROM t\nORDER BY a ASC"
    );
    assert_eq!(
        sorted("SELECT * FROM t -- order by z\n/* limit 1 */", PG),
        "SELECT * FROM t\nORDER BY a ASC -- order by z\n/* limit 1 */"
    );
}

#[test]
fn keeps_a_trailing_semicolon() {
    assert_eq!(
        sorted("SELECT * FROM t;", PG),
        "SELECT * FROM t\nORDER BY a ASC;"
    );
    assert_eq!(
        sorted("SELECT * FROM t ORDER BY b;\n", PG),
        "SELECT * FROM t ORDER BY a ASC;\n"
    );
}

#[test]
fn quotes_the_column_per_dialect() {
    let q = "SELECT * FROM t";
    assert_eq!(
        sort(q, PG, "Full Name", false).unwrap(),
        "SELECT * FROM t\nORDER BY \"Full Name\" ASC"
    );
    assert_eq!(
        sort(q, MY, "Full Name", true).unwrap(),
        "SELECT * FROM t\nORDER BY `Full Name` DESC"
    );
    assert_eq!(
        sort(q, MS, "Full Name", false).unwrap(),
        "SELECT * FROM t\nORDER BY [Full Name] ASC"
    );
    assert_eq!(
        sort(q, Dialect::Sqlite, "order", false).unwrap(),
        "SELECT * FROM t\nORDER BY \"order\" ASC"
    );
}

#[test]
fn counts_a_plain_query_without_a_derived_table() {
    assert_eq!(
        count("SELECT * FROM t WHERE a = 1 ORDER BY b DESC;", PG),
        "SELECT COUNT(*) FROM t WHERE a = 1"
    );
    // Duplicate column names in a join would fail in a derived table.
    for dialect in [MY, MS, Dialect::BigQuery] {
        assert_eq!(
            count(
                "SELECT * FROM a JOIN b ON a.id = b.id ORDER BY a.id",
                dialect
            ),
            "SELECT COUNT(*) FROM a JOIN b ON a.id = b.id"
        );
    }
    assert_eq!(
        count("SELECT * FROM t -- last\n", Dialect::Oracle),
        "SELECT COUNT(*) FROM t"
    );
    assert_eq!(
        count("SELECT * FROM t WHERE a = 1 FOR UPDATE", PG),
        "SELECT COUNT(*) FROM t WHERE a = 1"
    );
    assert_eq!(
        count("SELECT * FROM t OPTION (RECOMPILE)", MS),
        "SELECT COUNT(*) FROM t"
    );
    assert_eq!(
        count("SELECT * FROM t LOCK IN SHARE MODE", MY),
        "SELECT COUNT(*) FROM t"
    );
}

#[test]
fn counts_after_a_cte_without_wrapping_it() {
    assert_eq!(
        count("WITH c AS (SELECT 1 AS id) SELECT * FROM c ORDER BY id", MS),
        "WITH c AS (SELECT 1 AS id) SELECT COUNT(*) FROM c"
    );
    // A query that needs the wrapper keeps the CTE outside, as Transact-SQL requires.
    assert_eq!(
        count("WITH c AS (SELECT 1 AS id) SELECT DISTINCT id FROM c", MS),
        "WITH c AS (SELECT 1 AS id) SELECT COUNT(*) FROM (\nSELECT DISTINCT id FROM c\n) t"
    );
    assert_eq!(
        count(
            "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM c WHERE n < 3) SELECT n FROM c LIMIT 2",
            PG
        ),
        "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM c WHERE n < 3) SELECT COUNT(*) FROM (\nSELECT 1 FROM c LIMIT 2\n) t"
    );
}

#[test]
fn wraps_what_a_plain_count_would_change() {
    assert_eq!(
        count("SELECT * FROM t ORDER BY a LIMIT 5", MY),
        "SELECT COUNT(*) FROM (\nSELECT 1 FROM t LIMIT 5\n) t"
    );
    assert_eq!(
        count("SELECT TOP (5) * FROM t ORDER BY a", MS),
        "SELECT COUNT(*) FROM (\nSELECT TOP (5) 1 FROM t\n) t"
    );
    assert_eq!(
        count("SELECT DISTINCT a FROM t ORDER BY a", PG),
        "SELECT COUNT(*) FROM (\nSELECT DISTINCT a FROM t\n) t"
    );
    assert_eq!(
        count("SELECT a, count(*) FROM t GROUP BY a ORDER BY a", PG),
        "SELECT COUNT(*) FROM (\nSELECT 1 FROM t GROUP BY a\n) t"
    );
    assert_eq!(
        count("SELECT a, count(*) c FROM t GROUP BY a HAVING c > 1", MY),
        "SELECT COUNT(*) FROM (\nSELECT a, count(*) c FROM t GROUP BY a HAVING c > 1\n) t"
    );
    assert_eq!(
        count("SELECT count(*) FROM t", PG),
        "SELECT COUNT(*) FROM (\nSELECT count(*) FROM t\n) t"
    );
    assert_eq!(
        count("SELECT row_number() OVER (ORDER BY a) FROM t", PG),
        "SELECT COUNT(*) FROM (\nSELECT 1 FROM t\n) t"
    );
    assert_eq!(
        count("SELECT a FROM x UNION SELECT a FROM y ORDER BY a", PG),
        "SELECT COUNT(*) FROM (\nSELECT a FROM x UNION SELECT a FROM y\n) t"
    );
    assert_eq!(
        count("SELECT 1", PG),
        "SELECT COUNT(*) FROM (\nSELECT 1\n) t"
    );
}

#[test]
fn counts_with_the_alias_each_dialect_accepts() {
    // Oracle has no AS before a table alias: none is used anywhere.
    let oracle = count(
        "SELECT * FROM t ORDER BY a FETCH FIRST 5 ROWS ONLY",
        Dialect::Oracle,
    );
    assert_eq!(
        oracle,
        "SELECT COUNT(*) FROM (\nSELECT 1 FROM t FETCH FIRST 5 ROWS ONLY\n) t"
    );
    assert!(!oracle.contains(" AS t"));
    // Transact-SQL needs the ORDER BY that OFFSET … FETCH belongs to.
    assert_eq!(
        count(
            "SELECT * FROM t ORDER BY a OFFSET 1 ROWS FETCH NEXT 2 ROWS ONLY",
            MS
        ),
        "SELECT COUNT(*) FROM (\nSELECT * FROM t ORDER BY a OFFSET 1 ROWS FETCH NEXT 2 ROWS ONLY\n) t"
    );
    assert_eq!(
        count("SELECT * FROM t ORDER BY a OFFSET 1 ROWS", PG),
        "SELECT COUNT(*) FROM (\nSELECT 1 FROM t OFFSET 1 ROWS\n) t"
    );
}

#[test]
fn adds_a_where() {
    assert_eq!(
        filtered("SELECT * FROM t ORDER BY a LIMIT 3", PG, "a = 1"),
        "SELECT * FROM t\nWHERE a = 1 ORDER BY a LIMIT 3"
    );
    assert_eq!(
        filtered("SELECT a FROM t GROUP BY a", PG, "x = 1"),
        "SELECT a FROM t\nWHERE x = 1 GROUP BY a"
    );
    assert_eq!(
        filtered("SELECT * FROM t FOR UPDATE", PG, "a = 1"),
        "SELECT * FROM t\nWHERE a = 1 FOR UPDATE"
    );
    assert_eq!(
        filtered("WITH c AS (SELECT 1 a) SELECT * FROM c;", PG, "a = 1"),
        "WITH c AS (SELECT 1 a) SELECT * FROM c\nWHERE a = 1;"
    );
    assert_eq!(
        filtered("SELECT * FROM (SELECT * FROM u WHERE z = 1) s", PG, "a = 1"),
        "SELECT * FROM (SELECT * FROM u WHERE z = 1) s\nWHERE a = 1"
    );
}

#[test]
fn ands_into_an_existing_where() {
    assert_eq!(
        filtered("SELECT * FROM t WHERE a = 1 ORDER BY a", PG, "b > 2"),
        "SELECT * FROM t WHERE a = 1 AND b > 2 ORDER BY a"
    );
    assert_eq!(
        filtered("SELECT * FROM t WHERE a = 1 OR b = 2", PG, "c = 3"),
        "SELECT * FROM t WHERE (a = 1 OR b = 2) AND c = 3"
    );
    // An OR in parentheses, or in a string, does not bind looser than the AND.
    assert_eq!(
        filtered(
            "SELECT * FROM t WHERE (a = 1 OR b = 2) AND s = 'x or y'",
            PG,
            "c = 3"
        ),
        "SELECT * FROM t WHERE (a = 1 OR b = 2) AND s = 'x or y' AND c = 3"
    );
    assert_eq!(
        filtered(
            "SELECT * FROM a JOIN b ON a.x = b.x OR a.y = b.y WHERE a.z = 1",
            PG,
            "a.w = 2"
        ),
        "SELECT * FROM a JOIN b ON a.x = b.x OR a.y = b.y WHERE a.z = 1 AND a.w = 2"
    );
}

#[test]
fn adds_no_condition_where_it_cannot_tell() {
    assert_eq!(
        add_condition("SELECT a FROM x UNION SELECT a FROM y", PG, "a = 1"),
        None
    );
    assert_eq!(add_condition("SELECT 1", PG, "a = 1"), None);
    assert_eq!(add_condition("SELECT * FROM t", PG, "  "), None);
}
