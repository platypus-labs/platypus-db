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
fn sorts_by_position_where_a_name_is_ambiguous() {
    let join = "SELECT a.id, b.id FROM a JOIN b ON b.a_id = a.id";
    assert_eq!(
        set_order_by_position(join, PG, 2, true).unwrap(),
        format!("{join}\nORDER BY 2 DESC")
    );
    assert_eq!(
        set_order_by_position("SELECT TOP (5) a.*, b.name FROM a, b ORDER BY a.id", MS, 1, false)
            .unwrap(),
        "SELECT TOP (5) a.*, b.name FROM a, b ORDER BY 1 ASC"
    );
    assert_eq!(
        set_order_by_position("SELECT count(*) FROM t LIMIT 3", MY, 1, false).unwrap(),
        "SELECT count(*) FROM t\nORDER BY 1 ASC LIMIT 3"
    );
    assert_eq!(set_order_by_position(join, PG, 0, false), None);
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
fn reads_the_order_by_as_written() {
    assert_eq!(
        order_by_clause(
            "SELECT * FROM t WHERE a = 1\n  ORDER BY name /* why */ DESC, id LIMIT 3;",
            PG
        )
        .as_deref(),
        Some("ORDER BY name /* why */ DESC, id")
    );
    assert_eq!(
        order_by_clause("SELECT * FROM (SELECT * FROM t ORDER BY a) s", PG),
        None
    );
    assert_eq!(order_by_clause("SELECT * FROM t; SELECT 1", PG), None);
}

#[test]
fn restores_the_order_by_the_statement_had() {
    for (sql, dialect) in [
        ("SELECT * FROM t\n  ORDER BY name DESC", PG),
        (
            "SELECT * FROM t ORDER BY name /* why */ DESC, id LIMIT 3",
            MY,
        ),
        (
            "SELECT * FROM t ORDER BY name DESC OFFSET 1 ROWS FETCH NEXT 2 ROWS ONLY",
            MS,
        ),
        ("SELECT TOP (3) WITH TIES * FROM t ORDER BY name DESC", MS),
    ] {
        let clause = order_by_clause(sql, dialect).unwrap();
        let sorted = sort(sql, dialect, "id", true).unwrap();
        assert_eq!(
            restore_order_by(&sorted, dialect, &clause).as_deref(),
            Some(sql),
            "{sql}"
        );
    }
    // A condition added since stays; a statement without a sort gets the clause back too.
    let filtered = filtered("SELECT * FROM t\nORDER BY id ASC", PG, "a = 1");
    assert_eq!(
        restore_order_by(&filtered, PG, " ORDER BY name DESC ").as_deref(),
        Some("SELECT * FROM t\nWHERE a = 1\nORDER BY name DESC")
    );
    assert_eq!(
        restore_order_by("SELECT * FROM t LIMIT 3", PG, "ORDER BY name DESC").as_deref(),
        Some("SELECT * FROM t\nORDER BY name DESC LIMIT 3")
    );
}

#[test]
fn counts_the_distinct_rows_of_oracle_unique() {
    assert_eq!(
        count("SELECT UNIQUE a FROM t", Dialect::Oracle),
        "SELECT COUNT(*) FROM (
SELECT UNIQUE a FROM t
) t"
    );
}

#[test]
fn restores_only_one_order_by() {
    let sql = "SELECT * FROM t ORDER BY id ASC";
    for clause in [
        "ORDER BY a; DELETE FROM t",
        "LIMIT 3",
        "ORDER BY a)",
        "",
        "ORDER BY a UNION SELECT * FROM u",
    ] {
        assert_eq!(restore_order_by(sql, PG, clause), None, "{clause}");
    }
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
        "SELECT COUNT(*) FROM t\nOPTION (RECOMPILE)"
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
        "WITH c AS (SELECT 1 AS id) SELECT COUNT(*) FROM (\nSELECT DISTINCT id FROM c\n) t (c1)"
    );
    assert_eq!(
        count(
            "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM c WHERE n < 3) SELECT n FROM c LIMIT 2",
            PG
        ),
        "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM c WHERE n < 3) SELECT COUNT(*) FROM (\nSELECT 1 AS x FROM c LIMIT 2\n) t"
    );
}

#[test]
fn wraps_what_a_plain_count_would_change() {
    assert_eq!(
        count("SELECT * FROM t ORDER BY a LIMIT 5", MY),
        "SELECT COUNT(*) FROM (\nSELECT 1 AS x FROM t LIMIT 5\n) t"
    );
    assert_eq!(
        count("SELECT TOP (5) * FROM t ORDER BY a", MS),
        "SELECT COUNT(*) FROM (\nSELECT TOP (5) 1 AS x FROM t\n) t"
    );
    assert_eq!(
        count("SELECT DISTINCT a FROM t ORDER BY a", PG),
        "SELECT COUNT(*) FROM (\nSELECT DISTINCT a FROM t\n) t"
    );
    // A GROUP BY may name a column of the select list (`GROUP BY 1`): the list stays.
    assert_eq!(
        count("SELECT a, count(*) FROM t GROUP BY 1 ORDER BY a", PG),
        "SELECT COUNT(*) FROM (\nSELECT a, count(*) FROM t GROUP BY 1\n) t"
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
        "SELECT COUNT(*) FROM (\nSELECT 1 AS x FROM t\n) t"
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
        "SELECT COUNT(*) FROM (\nSELECT 1 AS x FROM t FETCH FIRST 5 ROWS ONLY\n) t"
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
        "SELECT COUNT(*) FROM (\nSELECT 1 AS x FROM t OFFSET 1 ROWS\n) t"
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
fn sorts_and_counts_every_set_operation() {
    for op in ["INTERSECT", "EXCEPT", "UNION ALL"] {
        let sql = format!("SELECT a FROM x {op} SELECT a FROM y");
        assert_eq!(sorted(&sql, PG), format!("{sql}\nORDER BY a ASC"));
        assert_eq!(
            count(&format!("{sql} ORDER BY a"), PG),
            format!("SELECT COUNT(*) FROM (\n{sql}\n) t")
        );
    }
    // An operand in parentheses keeps its own ORDER BY and LIMIT.
    let sql = "SELECT a FROM x UNION (SELECT a FROM y ORDER BY a LIMIT 1)";
    assert_eq!(
        sort(sql, PG, "b", true).unwrap(),
        format!("{sql}\nORDER BY b DESC")
    );
    assert_eq!(
        sort(&format!("{sql} ORDER BY 1"), PG, "b", true).unwrap(),
        format!("{sql} ORDER BY b DESC")
    );
    assert_eq!(unsorted(&format!("{sql} ORDER BY 1"), PG), sql);
}

#[test]
fn removing_the_sort_keeps_it_for_offset_alone_on_transact_sql() {
    assert_eq!(
        unsorted("SELECT * FROM t ORDER BY a OFFSET 1 ROWS", MS),
        "SELECT * FROM t ORDER BY (SELECT NULL) OFFSET 1 ROWS"
    );
    // TOP … WITH TIES needs one too.
    assert_eq!(
        unsorted("SELECT TOP (2) WITH TIES * FROM t ORDER BY a", MS),
        "SELECT TOP (2) WITH TIES * FROM t ORDER BY (SELECT NULL)"
    );
    assert_eq!(
        count("SELECT TOP (2) WITH TIES * FROM t ORDER BY a", MS),
        "SELECT COUNT(*) FROM (\nSELECT TOP (2) WITH TIES * FROM t ORDER BY a\n) t"
    );
    // Elsewhere FETCH … WITH TIES keeps its ORDER BY: it can change, not go.
    let ties = "SELECT * FROM t ORDER BY a FETCH FIRST 2 ROWS WITH TIES";
    assert_eq!(set_order_by(ties, PG, None), None);
    assert_eq!(
        sort(ties, PG, "b", true).unwrap(),
        "SELECT * FROM t ORDER BY b DESC FETCH FIRST 2 ROWS WITH TIES"
    );
    assert_eq!(
        count(ties, Dialect::Oracle),
        format!("SELECT COUNT(*) FROM (\n{ties}\n) t")
    );
}

#[test]
fn gives_up_on_an_empty_statement_around_the_query() {
    for sql in [
        ";SELECT 1",
        "; SELECT * FROM t",
        "SELECT * FROM t;;",
        "  ;  ",
    ] {
        assert_eq!(sort(sql, PG, "a", false), None, "{sql}");
        assert_eq!(count_query(sql, PG), None, "{sql}");
        assert_eq!(add_condition(sql, PG, "a = 1"), None, "{sql}");
    }
}

#[test]
fn a_clause_word_used_as_a_column_name_is_a_column() {
    // MySQL and SQLite accept `offset` and `limit`-like words as bare column names.
    assert_eq!(
        sorted("SELECT offset FROM t", MY),
        "SELECT offset FROM t\nORDER BY a ASC"
    );
    assert_eq!(
        sorted("SELECT id, offset FROM t LIMIT 3", MY),
        "SELECT id, offset FROM t\nORDER BY a ASC LIMIT 3"
    );
    assert_eq!(count("SELECT offset FROM t", MY), "SELECT COUNT(*) FROM t");
    assert_eq!(
        sorted(
            "SELECT * FROM t WHERE offset > 1 AND t.limit < 2 LIMIT 3",
            MY
        ),
        "SELECT * FROM t WHERE offset > 1 AND t.limit < 2\nORDER BY a ASC LIMIT 3"
    );
    assert_eq!(
        filtered("SELECT a AS offset FROM t ORDER BY offset", MY, "a > 1"),
        "SELECT a AS offset FROM t\nWHERE a > 1 ORDER BY offset"
    );
    // A query without FROM still has its clauses, and so do parameters and LIMIT ALL.
    assert_eq!(
        sorted("SELECT 1 LIMIT 1", PG),
        "SELECT 1\nORDER BY a ASC LIMIT 1"
    );
    assert_eq!(
        sorted("SELECT * FROM t LIMIT ? OFFSET ?", MY),
        "SELECT * FROM t\nORDER BY a ASC LIMIT ? OFFSET ?"
    );
    assert_eq!(
        sorted("SELECT * FROM t LIMIT :n OFFSET :m", PG),
        "SELECT * FROM t\nORDER BY a ASC LIMIT :n OFFSET :m"
    );
    assert_eq!(
        sorted("SELECT * FROM t LIMIT ALL OFFSET 5", PG),
        "SELECT * FROM t\nORDER BY a ASC LIMIT ALL OFFSET 5"
    );
}

#[test]
fn does_not_sort_a_distinct_on_by_another_column() {
    // PostgreSQL wants the ORDER BY to start with the DISTINCT ON expressions.
    let sql = "SELECT DISTINCT ON (a) * FROM t ORDER BY a, b";
    assert_eq!(sort(sql, PG, "c", false), None);
    assert_eq!(set_order_by(sql, PG, None), None);
    assert_eq!(
        count(sql, PG),
        "SELECT COUNT(*) FROM (\nSELECT DISTINCT ON (a) * FROM t\n) t"
    );
    assert_eq!(
        filtered(sql, PG, "c = 1"),
        "SELECT DISTINCT ON (a) * FROM t\nWHERE c = 1 ORDER BY a, b"
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

#[test]
fn names_every_column_of_a_count_on_transact_sql() {
    // Transact-SQL refuses a derived table with an unnamed column (Msg 8155) or a name twice.
    assert_eq!(
        count("SELECT TOP (5) a + 1 FROM t ORDER BY a", MS),
        "SELECT COUNT(*) FROM (\nSELECT TOP (5) 1 AS x FROM t\n) t"
    );
    // The lexer reads `10` as two symbols: the select list starts after both.
    assert_eq!(
        count("SELECT TOP 10 a FROM t", MS),
        "SELECT COUNT(*) FROM (\nSELECT TOP 10 1 AS x FROM t\n) t"
    );
    assert_eq!(
        count("SELECT a, count(*) FROM t GROUP BY a", MS),
        "SELECT COUNT(*) FROM (\nSELECT a, count(*) FROM t GROUP BY a\n) t (c1, c2)"
    );
    assert_eq!(
        count(
            "SELECT DISTINCT a.id, b.id FROM a JOIN b ON a.id = b.id",
            MS
        ),
        "SELECT COUNT(*) FROM (\nSELECT DISTINCT a.id, b.id FROM a JOIN b ON a.id = b.id\n) t (c1, c2)"
    );
    assert_eq!(
        count(
            "SELECT a, b * 2 FROM t ORDER BY a OFFSET 1 ROWS FETCH NEXT 2 ROWS ONLY",
            MS
        ),
        "SELECT COUNT(*) FROM (\nSELECT a, b * 2 FROM t ORDER BY a OFFSET 1 ROWS FETCH NEXT 2 ROWS ONLY\n) t (c1, c2)"
    );
    assert_eq!(
        count("SELECT a, count(*) OVER () FROM t", MS),
        "SELECT COUNT(*) FROM (\nSELECT a, count(*) OVER () FROM t\n) t (c1, c2)"
    );
    assert_eq!(
        count("SELECT count(*) FROM t", MS),
        "SELECT COUNT(*) FROM (\nSELECT count(*) FROM t\n) t (c1)"
    );
    assert_eq!(
        count("SELECT 1, 2", MS),
        "SELECT COUNT(*) FROM (\nSELECT 1, 2\n) t (c1, c2)"
    );
    // `*` hides how many columns there are: the query's own names have to do.
    assert_eq!(
        count("SELECT DISTINCT * FROM t", MS),
        "SELECT COUNT(*) FROM (\nSELECT DISTINCT * FROM t\n) t"
    );
    assert_eq!(
        count("SELECT DISTINCT a, b FROM t", PG),
        "SELECT COUNT(*) FROM (\nSELECT DISTINCT a, b FROM t\n) t"
    );
}

#[test]
fn keeps_option_at_the_end_of_a_count() {
    // `OPTION (MAXRECURSION 0)` lets a recursive CTE go deeper than 100 levels.
    let cte = "WITH c(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM c WHERE n < 200)";
    assert_eq!(
        count(
            &format!("{cte} SELECT n FROM c ORDER BY n OPTION (MAXRECURSION 0)"),
            MS
        ),
        format!("{cte} SELECT COUNT(*) FROM c\nOPTION (MAXRECURSION 0)")
    );
    assert_eq!(
        count("SELECT DISTINCT a FROM t OPTION (MAXDOP 1)", MS),
        "SELECT COUNT(*) FROM (\nSELECT DISTINCT a FROM t\n) t (c1)\nOPTION (MAXDOP 1)"
    );
}

#[test]
fn a_table_or_column_named_like_a_clause_is_a_name() {
    for dialect in [PG, MY, MS] {
        let sql = "SELECT * FROM settings";
        assert_eq!(sorted(sql, dialect), format!("{sql}\nORDER BY a ASC"));
        assert_eq!(count(sql, dialect), "SELECT COUNT(*) FROM settings");
        assert_eq!(
            filtered(sql, dialect, "a = 1"),
            format!("{sql}\nWHERE a = 1")
        );
        let sql = "SELECT id, format FROM files";
        assert_eq!(sorted(sql, dialect), format!("{sql}\nORDER BY a ASC"));
        assert_eq!(count(sql, dialect), "SELECT COUNT(*) FROM files");
        assert_eq!(
            filtered("SELECT * FROM files WHERE format = 'pdf'", dialect, "a = 1"),
            "SELECT * FROM files WHERE format = 'pdf' AND a = 1"
        );
        assert_eq!(
            count("SELECT procedure FROM t", dialect),
            "SELECT COUNT(*) FROM t"
        );
    }
    assert_eq!(
        sorted("SELECT * FROM offset LIMIT 2", MY),
        "SELECT * FROM offset\nORDER BY a ASC LIMIT 2"
    );
    assert_eq!(
        count("SELECT format FROM t", Dialect::ClickHouse),
        "SELECT COUNT(*) FROM t"
    );
    // The clauses themselves still stop the rewrite.
    assert_eq!(count_query("SELECT * FROM t PROCEDURE ANALYSE()", MY), None);
}

#[test]
fn gives_up_on_a_clause_out_of_place() {
    // Invalid SQL, but "Count rows" may get it before it ever ran.
    let cases = [
        ("SELECT * FROM t FOR UPDATE ORDER BY a", PG),
        ("SELECT * FROM t LIMIT 1 ORDER BY a", PG),
        ("SELECT * FROM t ORDER BY a ORDER BY b", PG),
        ("SELECT * FROM t ORDER BY a GROUP BY b", PG),
        ("SELECT * FROM t LIMIT 1 UNION SELECT * FROM u", MY),
        ("SELECT * FROM t LOCK IN SHARE MODE ORDER BY a", MY),
        ("SELECT * FROM t OPTION (RECOMPILE) ORDER BY a", MS),
        ("SELECT * FROM t OPTION (RECOMPILE) OPTION (MAXDOP 1)", MS),
    ];
    for (sql, dialect) in cases {
        assert_eq!(sort(sql, dialect, "a", false), None, "{sql}");
        assert_eq!(set_order_by(sql, dialect, None), None, "{sql}");
        assert_eq!(count_query(sql, dialect), None, "{sql}");
        assert_eq!(add_condition(sql, dialect, "a = 1"), None, "{sql}");
    }
    // PostgreSQL takes a lock clause before LIMIT: the count drops only the lock.
    let sql = "SELECT * FROM t FOR UPDATE LIMIT 1";
    assert_eq!(
        count(sql, PG),
        "SELECT COUNT(*) FROM (\nSELECT 1 AS x FROM t LIMIT 1\n) t"
    );
    assert_eq!(
        sorted(sql, PG),
        "SELECT * FROM t\nORDER BY a ASC FOR UPDATE LIMIT 1"
    );
}

#[test]
fn parenthesises_a_condition_with_an_operator_looser_than_and() {
    assert_eq!(
        filtered("SELECT * FROM t WHERE a = 1 || b = 2", MY, "c = 3"),
        "SELECT * FROM t WHERE (a = 1 || b = 2) AND c = 3"
    );
    assert_eq!(
        filtered("SELECT * FROM t WHERE a = 1 XOR b = 2", MY, "c = 3"),
        "SELECT * FROM t WHERE (a = 1 XOR b = 2) AND c = 3"
    );
    // Elsewhere `||` joins strings.
    assert_eq!(
        filtered("SELECT * FROM t WHERE a || b = 'xy'", PG, "c = 3"),
        "SELECT * FROM t WHERE a || b = 'xy' AND c = 3"
    );
}

#[test]
fn is_distinct_from_is_not_the_from_clause() {
    assert_eq!(
        count("SELECT a IS DISTINCT FROM b AS x FROM t", PG),
        "SELECT COUNT(*) FROM t"
    );
    assert_eq!(
        filtered("SELECT a IS NOT DISTINCT FROM b FROM t", PG, "c = 1"),
        "SELECT a IS NOT DISTINCT FROM b FROM t\nWHERE c = 1"
    );
}

#[test]
fn counts_one_row_for_an_aggregate_of_any_dialect() {
    for (sql, dialect, columns) in [
        ("SELECT jsonb_object_agg(k, v) FROM t", PG, ""),
        (
            "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY a) FROM t",
            PG,
            "",
        ),
        ("SELECT stdev(a) FROM t", MS, " (c1)"),
        ("SELECT countIf(a > 1) FROM t", Dialect::ClickHouse, ""),
        (
            "SELECT approx_count_distinct(a) FROM t",
            Dialect::Snowflake,
            "",
        ),
    ] {
        assert_eq!(
            count(sql, dialect),
            format!("SELECT COUNT(*) FROM (\n{sql}\n) t{columns}"),
            "{sql}"
        );
    }
}

/// Every dialect, for the tests that run on all of them.
const DIALECTS: [Dialect; 12] = [
    Dialect::Postgres,
    Dialect::MySql,
    Dialect::Sqlite,
    Dialect::DuckDb,
    Dialect::SqlServer,
    Dialect::ClickHouse,
    Dialect::Ansi,
    Dialect::Snowflake,
    Dialect::BigQuery,
    Dialect::Databricks,
    Dialect::Oracle,
    Dialect::Db2,
];

/// Pieces of SQL, valid or not, that `never_panics_on_odd_input` strings together.
const FRAGMENTS: &[&str] = &[
    "SELECT",
    "*",
    "a",
    "t.*",
    "FROM",
    "t",
    ",",
    "(",
    ")",
    ";",
    "WHERE",
    "a = 1",
    "OR",
    "||",
    "|",
    "XOR",
    "GROUP BY",
    "HAVING",
    "ORDER BY",
    "ORDER",
    "BY",
    "LIMIT",
    "1",
    "10",
    "OFFSET",
    "FETCH FIRST 1 ROWS",
    "WITH TIES",
    "TOP",
    "TOP (2)",
    "DISTINCT",
    "DISTINCT ON (a)",
    "FOR UPDATE",
    "FOR",
    "LOCK IN SHARE MODE",
    "OPTION (RECOMPILE)",
    "OPTION",
    "UNION",
    "WITH c AS (SELECT 1)",
    "WITH",
    "INTO",
    "SETTINGS",
    "FORMAT",
    "PROCEDURE",
    "count(*)",
    "OVER ()",
    "IS DISTINCT FROM",
    "'città'",
    "città",
    "\u{a0}",
    "--",
    "/*",
    "*/",
    "'",
    "\"",
    "`",
    "[",
    "]",
    "$$",
    "$1",
    "?",
    ":n",
    "\n",
    "🦆",
    "settings",
    "offset",
    "#",
    "@",
    "N'é'",
    "q'[x]'",
    "e'\\'",
];

#[test]
fn never_panics_on_odd_input() {
    // A fixed linear congruential sequence: the same inputs on every run.
    let mut seed = 0x9e37_79b9_u32;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (seed >> 16) as usize % n
    };
    for _ in 0..3000 {
        let mut sql = String::from(if next(4) == 0 { "" } else { "SELECT " });
        for _ in 0..=next(10) {
            sql.push_str(FRAGMENTS[next(FRAGMENTS.len())]);
            if next(4) != 0 {
                sql.push(' ');
            }
        }
        for dialect in DIALECTS {
            let _ = set_order_by(&sql, dialect, Some(("a", true)));
            let _ = set_order_by(&sql, dialect, None);
            let _ = count_query(&sql, dialect);
            let _ = add_condition(&sql, dialect, "a = 1");
            let _ = filter_columns(&sql, dialect, &["a", "b"]);
            let _ = restore_order_by(&sql, dialect, "ORDER BY a");
        }
    }
}

#[test]
fn adds_a_row_cap_to_queries_without_one() {
    let limited = |sql, dialect| add_limit(sql, dialect, 100);
    assert_eq!(
        limited("SELECT * FROM t ORDER BY a;", PG).as_deref(),
        Some("SELECT * FROM t ORDER BY a LIMIT 100;")
    );
    assert_eq!(
        limited("WITH x AS (SELECT 1 LIMIT 5) SELECT * FROM x", PG).as_deref(),
        Some("WITH x AS (SELECT 1 LIMIT 5) SELECT * FROM x LIMIT 100")
    );
    assert_eq!(
        limited("SELECT a FROM t UNION SELECT b FROM u", MY).as_deref(),
        Some("SELECT a FROM t UNION SELECT b FROM u LIMIT 100")
    );
    assert_eq!(
        limited("SELECT DISTINCT a FROM t ORDER BY a OPTION (RECOMPILE)", MS).as_deref(),
        Some("SELECT DISTINCT TOP (100) a FROM t ORDER BY a OPTION (RECOMPILE)")
    );
    assert_eq!(
        limited("select * from t", Dialect::Oracle).as_deref(),
        Some("select * from t FETCH FIRST 100 ROWS ONLY")
    );
}

#[test]
fn leaves_capped_and_other_statements_alone() {
    for (sql, dialect) in [
        ("SELECT * FROM t LIMIT 5", PG),
        ("SELECT * FROM t OFFSET 10", PG),
        ("SELECT TOP 5 * FROM t", MS),
        ("SELECT a FROM t UNION SELECT b FROM u", MS),
        (
            "SELECT a FROM t UNION ALL SELECT b FROM u",
            Dialect::ClickHouse,
        ),
        ("SELECT * FROM t FOR UPDATE", MY),
        ("SELECT 1", PG),
        ("SELECT * INTO t2 FROM t", PG),
        ("UPDATE t SET a = 1", PG),
        ("SELECT 1 FROM t; SELECT 2 FROM t", PG),
    ] {
        assert_eq!(add_limit(sql, dialect, 100), None, "{sql}");
    }
}
