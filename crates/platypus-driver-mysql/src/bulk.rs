//! Bulk loading with `LOAD DATA LOCAL INFILE`: the rows go to the server as the "file" it asks
//! for, tab separated. `LOCAL` turns bad values and duplicate keys into warnings and skips
//! those rows; any warning fails the load here, so that the caller's transaction undoes it.

use bytes::Bytes;
use futures::StreamExt;
use mysql_async::prelude::Queryable;
use platypus_core::{BulkRows, Error, Result, TxEffect};

use crate::{MySqlConnection, map_error};

/// The server or the client does not allow `LOCAL INFILE` (`local_infile` off).
const NOT_ALLOWED: [u16; 2] = [1148, 3948];

pub async fn load(conn: &MySqlConnection, rows: &BulkRows<'_>) -> Result<u64> {
    let data = Bytes::from(tab_separated(rows));
    // Bytes columns arrive as hex in a variable, then UNHEX'd.
    let mut targets = Vec::with_capacity(rows.columns.len());
    let mut set = Vec::new();
    for (i, column) in rows.columns.iter().enumerate() {
        if rows.hex.get(i).copied().unwrap_or(false) {
            targets.push(format!("@h{i}"));
            set.push(format!("{column} = UNHEX(@h{i})"));
        } else {
            targets.push(column.clone());
        }
    }
    let sql = format!(
        "LOAD DATA LOCAL INFILE 'platypus-rows' INTO TABLE {} CHARACTER SET utf8mb4 \
         FIELDS TERMINATED BY {} ESCAPED BY {} LINES TERMINATED BY {} ({}){}",
        rows.table,
        conn.literal("\t"),
        conn.literal("\\"),
        conn.literal("\n"),
        targets.join(", "),
        if set.is_empty() {
            String::new()
        } else {
            format!(" SET {}", set.join(", "))
        }
    );
    let mut session = conn.conn.lock().await;
    session.set_infile_handler(async move {
        Ok(futures::stream::iter([Ok::<_, std::io::Error>(data)]).boxed())
    });
    let ran = session.query_drop(&sql).await;
    // In manual-commit mode the load opened the transaction, as a statement does.
    conn.tx.after(TxEffect::None, true);
    match ran {
        Err(mysql_async::Error::Server(e)) if NOT_ALLOWED.contains(&e.code) => {
            return Err(Error::Unsupported(format!(
                "LOAD DATA LOCAL is not allowed: {}",
                e.message
            )));
        }
        other => other.map_err(map_error)?,
    }
    let loaded = session.affected_rows();
    let warnings: Vec<(String, u32, String)> = session
        .query("SHOW WARNINGS LIMIT 3")
        .await
        .map_err(map_error)?;
    if let Some((_, code, message)) = warnings.first() {
        return Err(Error::Other(format!(
            "the rows were not loaded as given ({code}: {message})"
        )));
    }
    Ok(loaded)
}

/// The rows as `LOAD DATA` reads them: tab between values, a line each, `\N` for NULL, and
/// the backslash escapes of the default `ESCAPED BY`.
fn tab_separated(rows: &BulkRows<'_>) -> String {
    let mut out = String::with_capacity(rows.rows.len() * rows.columns.len() * 8);
    for row in rows.rows {
        for (i, value) in row.iter().enumerate() {
            if i > 0 {
                out.push('\t');
            }
            let Some(value) = value else {
                out.push_str("\\N");
                continue;
            };
            for c in value.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\t' => out.push_str("\\t"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\0' => out.push_str("\\0"),
                    c => out.push(c),
                }
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_rows_as_load_data_reads_them() {
        let rows = [
            vec![Some("1".to_owned()), None, Some(String::new())],
            vec![
                Some("a\tb\nc\\d".to_owned()),
                Some("N".to_owned()),
                Some("\0".to_owned()),
            ],
        ];
        let columns = ["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let bulk = BulkRows {
            table: "t",
            columns: &columns,
            hex: &[false; 3],
            rows: &rows,
        };
        assert_eq!(tab_separated(&bulk), "1\t\\N\t\na\\tb\\nc\\\\d\tN\t\\0\n");
    }
}
