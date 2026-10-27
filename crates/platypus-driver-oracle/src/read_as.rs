//! Queries whose columns the client library cannot read: `JSON` (21c) and `VECTOR` (23ai)
//! columns fail the whole query there, and a `BFILE` cannot be read as bytes. Such a query
//! runs again, once `DBMS_SQL` has described it, with those columns as text: `JSON_SERIALIZE`,
//! `FROM_VECTOR`, and for a file the `BFILENAME` call that makes its locator.

/// Describes the query bound to `:q` into `:r`: one line per column, its `DBMS_SQL` type code
/// and its name, apart by a tab.
pub const DESCRIBE: &str = "DECLARE c INTEGER := DBMS_SQL.OPEN_CURSOR; n INTEGER; \
     d DBMS_SQL.DESC_TAB3; r CLOB; BEGIN DBMS_SQL.PARSE(c, :q, DBMS_SQL.NATIVE); \
     DBMS_SQL.DESCRIBE_COLUMNS3(c, n, d); FOR i IN 1 .. n LOOP \
     r := r || d(i).col_type || CHR(9) || d(i).col_name || CHR(10); END LOOP; \
     DBMS_SQL.CLOSE_CURSOR(c); :r := r; EXCEPTION WHEN OTHERS THEN \
     IF DBMS_SQL.IS_OPEN(c) THEN DBMS_SQL.CLOSE_CURSOR(c); END IF; RAISE; END;";

/// `DBMS_SQL` type codes.
const JSON: i32 = 119;
const VECTOR: i32 = 127;
const BFILE: i32 = 114;

/// The function that writes a file locator as `BFILENAME('DIR', 'name')`: SQL has no
/// function for its directory and name, only `DBMS_LOB.FILEGETNAME`.
const FILE_NAME: &str = "WITH FUNCTION platypus_bfile(f BFILE) RETURN VARCHAR2 IS \
     d VARCHAR2(128); n VARCHAR2(4000); BEGIN IF f IS NULL THEN RETURN NULL; END IF; \
     DBMS_LOB.FILEGETNAME(f, d, n); RETURN 'BFILENAME(''' || REPLACE(d, '''', '''''') || \
     ''', ''' || REPLACE(n, '''', '''''') || ''')'; END;\n";

/// How to run a query again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retry {
    pub sql: String,
    /// The columns read as text, and the database type they have.
    pub labels: Vec<(usize, &'static str)>,
}

/// The columns of the lines [`DESCRIBE`] writes.
#[must_use]
pub fn described(text: &str) -> Vec<(i32, String)> {
    text.lines()
        .filter_map(|line| {
            let (code, name) = line.split_once('\t')?;
            Some((code.trim().parse().ok()?, name.to_owned()))
        })
        .collect()
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// How to run query `sql` again after `error` (`None`: it has a file column), with
/// `describe` telling its columns; `None` when the error is not one of a type the client
/// cannot read.
pub fn retry(
    sql: &str,
    error: Option<&oracle::Error>,
    describe: impl FnOnce(&str) -> Option<Vec<(i32, String)>>,
) -> Option<Retry> {
    if let Some(error) = error {
        let text = error.to_string();
        if !text.contains("unsupported Oracle type") && !text.contains("unknown Oracle type") {
            return None;
        }
    }
    rewrite(sql, &describe(sql)?)
}

/// `sql` reading `columns` as the client can: `None` when none needs it.
#[must_use]
pub fn rewrite(sql: &str, columns: &[(i32, String)]) -> Option<Retry> {
    let mut labels = Vec::new();
    let list: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, (code, name))| {
            let column = format!("q.{}", quoted(name));
            let (read, label) = match *code {
                JSON => (format!("JSON_SERIALIZE({column} RETURNING CLOB)"), "JSON"),
                VECTOR => (format!("FROM_VECTOR({column} RETURNING CLOB)"), "VECTOR"),
                BFILE => (format!("platypus_bfile({column})"), "BFILE"),
                _ => return column,
            };
            labels.push((i, label));
            format!("{read} {}", quoted(name))
        })
        .collect();
    if labels.is_empty() {
        return None;
    }
    let files = columns.iter().any(|(code, _)| *code == BFILE);
    Some(Retry {
        sql: format!(
            "{}SELECT {} FROM ({sql}\n) q",
            if files { FILE_NAME } else { "" },
            list.join(", ")
        ),
        labels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_json_vectors_and_files_as_text() {
        let columns = described("2\tID\n119\tJ\n127\tV\n");
        let retry = rewrite("SELECT * FROM T", &columns).unwrap();
        assert_eq!(
            retry.sql,
            "SELECT q.\"ID\", JSON_SERIALIZE(q.\"J\" RETURNING CLOB) \"J\", \
             FROM_VECTOR(q.\"V\" RETURNING CLOB) \"V\" FROM (SELECT * FROM T\n) q"
        );
        assert_eq!(retry.labels, [(1, "JSON"), (2, "VECTOR")]);
        let file = rewrite("SELECT F FROM T", &described("114\tF\n")).unwrap();
        assert!(
            file.sql.starts_with("WITH FUNCTION platypus_bfile"),
            "{}",
            file.sql
        );
        assert!(
            file.sql
                .ends_with("SELECT platypus_bfile(q.\"F\") \"F\" FROM (SELECT F FROM T\n) q")
        );
        assert_eq!(rewrite("SELECT 1 FROM DUAL", &described("2\t1\n")), None);
    }
}
