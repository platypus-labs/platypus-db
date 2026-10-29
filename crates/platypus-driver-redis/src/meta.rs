//! The catalog: schemas are the numbered databases (`db0`, `db1`…), each with a virtual table
//! per key type ([`KeyType`]).

use platypus_core::{ColumnMeta, Error, ObjectKind, ObjectMeta, Result, SchemaMeta, TableDetails};
use redis::Value;

use crate::command::KeyType;

/// Keys a navigator listing counts by type at most: past it the counts are left out.
pub const COUNT_CAP: usize = 10_000;

/// The schema name of database `n`.
#[must_use]
pub fn schema_name(n: i64) -> String {
    format!("db{n}")
}

/// The database of a schema name (`db3`, or `3`).
pub fn database(schema: &str) -> Result<i64> {
    let digits = schema.strip_prefix("db").unwrap_or(schema);
    digits
        .parse::<i64>()
        .ok()
        .filter(|n| *n >= 0)
        .ok_or_else(|| Error::Other(format!("unknown database `{schema}`")))
}

/// The databases with keys, from `INFO keyspace` (`db0:keys=3,expires=0,avg_ttl=0`).
#[must_use]
pub fn keyspace(info: &str) -> Vec<(i64, u64)> {
    info.lines()
        .filter_map(|line| {
            let (name, stats) = line.trim().split_once(':')?;
            let n = name.strip_prefix("db")?.parse().ok()?;
            let keys = stats
                .split(',')
                .find_map(|s| s.strip_prefix("keys="))?
                .parse()
                .ok()?;
            Some((n, keys))
        })
        .collect()
}

/// The databases with keys and the session's, in order.
#[must_use]
pub fn schemas(mut with_keys: Vec<i64>, current: i64) -> Vec<SchemaMeta> {
    if !with_keys.contains(&current) {
        with_keys.push(current);
    }
    with_keys.sort_unstable();
    with_keys.dedup();
    with_keys
        .into_iter()
        .map(|n| SchemaMeta {
            name: schema_name(n),
            system: false,
            current: n == current,
        })
        .collect()
}

/// The virtual tables, with the number of keys of each type as their comment when `counts`
/// has them (the scan went round within [`COUNT_CAP`]): their rows are the keys' values.
/// JSON shows only with the module or such keys.
#[must_use]
pub fn objects(counts: Option<&[(KeyType, u64)]>, json: bool) -> Vec<ObjectMeta> {
    KeyType::ALL
        .into_iter()
        .filter(|&kind| {
            kind != KeyType::Json
                || json
                || counts.is_some_and(|c| c.iter().any(|&(k, n)| k == kind && n > 0))
        })
        .map(|kind| {
            let keys = counts.map(|c| c.iter().find(|&&(k, _)| k == kind).map_or(0, |&(_, n)| n));
            let comment = keys.map(|n| {
                if n == 1 {
                    "1 key".to_owned()
                } else {
                    format!("{n} keys")
                }
            });
            ObjectMeta::new(kind.table(), ObjectKind::Table, comment)
        })
        .collect()
}

/// The keys of each type among `types`, the replies to `TYPE`.
#[must_use]
pub fn count_types(types: &[Value]) -> Vec<(KeyType, u64)> {
    let mut counts: Vec<(KeyType, u64)> = KeyType::ALL.into_iter().map(|k| (k, 0)).collect();
    for value in types {
        let name = match value {
            Value::SimpleString(s) => s.as_str(),
            Value::Okay => "OK",
            _ => continue,
        };
        if let Some(kind) = KeyType::from_redis(name)
            && let Some(entry) = counts.iter_mut().find(|(k, _)| *k == kind)
        {
            entry.1 += 1;
        }
    }
    counts
}

/// The columns of a virtual table.
pub fn table_details(table: &str) -> Result<TableDetails> {
    let kind = KeyType::from_table(table)
        .ok_or_else(|| Error::Other(format!("unknown table `{table}`")))?;
    let columns = kind
        .columns()
        .iter()
        .map(|&(name, key)| ColumnMeta {
            name: name.to_owned(),
            data_type: data_type(kind, name).to_owned(),
            nullable: name == "ttl",
            default: None,
            primary_key: key,
            comment: None,
            enum_values: Vec::new(),
            fields: Vec::new(),
        })
        .collect();
    Ok(TableDetails {
        columns,
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        constraints: Vec::new(),
        referenced_by: Vec::new(),
    })
}

/// The type a column shows in the catalog.
#[must_use]
pub fn data_type(kind: KeyType, column: &str) -> &'static str {
    match (kind, column) {
        (_, "ttl" | "index") => "integer",
        (KeyType::Zset, "score") => "double",
        (KeyType::Json, "value") => "json",
        (KeyType::Stream, "fields") => "document",
        _ => "string",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_keyspace() {
        let info = "# Keyspace\r\ndb0:keys=3,expires=1,avg_ttl=0\r\ndb5:keys=10,expires=0\r\n";
        assert_eq!(keyspace(info), [(0, 3), (5, 10)]);
        let names: Vec<_> = schemas(vec![5, 0], 2).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["db0", "db2", "db5"]);
        assert_eq!(database("db12").unwrap(), 12);
        assert_eq!(database("4").unwrap(), 4);
        assert!(database("x").is_err());
    }

    #[test]
    fn lists_a_table_per_type() {
        let names = |o: Vec<ObjectMeta>| o.into_iter().map(|o| o.name).collect::<Vec<_>>();
        assert!(!names(objects(None, false)).contains(&"json".to_owned()));
        assert!(names(objects(None, true)).contains(&"json".to_owned()));
        let counts = count_types(&[
            Value::SimpleString("hash".into()),
            Value::SimpleString("hash".into()),
            Value::SimpleString("ReJSON-RL".into()),
            Value::SimpleString("none".into()),
        ]);
        let objects = objects(Some(&counts), false);
        let hash = objects.iter().find(|o| o.name == "hash").unwrap();
        assert_eq!(hash.comment.as_deref(), Some("2 keys"));
        assert!(objects.iter().any(|o| o.name == "json"));
        let set = objects.iter().find(|o| o.name == "set").unwrap();
        assert_eq!(set.comment.as_deref(), Some("0 keys"));
    }

    #[test]
    fn describes_the_columns() {
        let details = table_details("zset").unwrap();
        let keys: Vec<_> = details
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(keys, ["key", "member"]);
        assert!(table_details("nope").is_err());
    }
}
