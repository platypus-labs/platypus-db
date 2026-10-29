//! The commands that save the grid's changes to a virtual table ([`KeyType`]), and those
//! that undo them. They start with `SELECT <db>`, which [`crate::RedisConnection`]'s
//! `execute_all` takes as the database of the batch, and run as one `MULTI`/`EXEC`.
//!
//! A row of a string or JSON table is a key: changing its `key` renames it. A row of the
//! other tables is an item of a key, which stays: its `key` cannot change. A new or renamed
//! key, field or member never replaces one that exists: the batch refuses it.

use crate::command::{KeyType, cell_bytes, line};

/// The editor line of a command: its name, then arguments as text or bytes.
macro_rules! cmd {
    ($name:literal $(, $arg:expr)* $(,)?) => {
        line(&[$name.as_bytes() $(, AsRef::<[u8]>::as_ref(&$arg))*])
    };
}

/// A loaded row changed: its values as loaded, and the new values of some columns (`None`
/// for NULL).
#[derive(Debug, Clone)]
pub struct Update {
    pub loaded: Vec<Option<String>>,
    pub set: Vec<(usize, Option<String>)>,
}

/// The changes to save to one virtual table.
#[derive(Debug, Clone)]
pub struct Edits<'a> {
    pub db: i64,
    pub kind: KeyType,
    /// The result's column names, by index.
    pub columns: &'a [String],
    pub updates: Vec<Update>,
    /// New rows: the values set, by column.
    pub inserts: Vec<Vec<(usize, Option<String>)>>,
    /// Deleted rows, as loaded.
    pub deletes: Vec<Vec<Option<String>>>,
}

/// The commands that save, and the script that puts things back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub commands: Vec<String>,
    /// Commands, and `#` comments for what cannot be undone.
    pub undo: Vec<String>,
}

/// A row's values by column name.
struct Row<'a> {
    columns: &'a [String],
    values: Vec<(usize, Option<&'a str>)>,
}

impl<'a> Row<'a> {
    fn loaded(columns: &'a [String], loaded: &'a [Option<String>]) -> Self {
        Self {
            columns,
            values: loaded
                .iter()
                .enumerate()
                .map(|(i, v)| (i, v.as_deref()))
                .collect(),
        }
    }

    fn set(columns: &'a [String], set: &'a [(usize, Option<String>)]) -> Self {
        Self {
            columns,
            values: set.iter().map(|(i, v)| (*i, v.as_deref())).collect(),
        }
    }

    /// The value of `column`: `None` when the row does not have it, `Some(None)` for NULL.
    #[expect(clippy::option_option, reason = "a column left out is not a NULL")]
    fn get(&self, column: &str) -> Option<Option<&'a str>> {
        let index = self.columns.iter().position(|c| c == column)?;
        self.values
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, v)| *v)
    }

    fn bytes(&self, column: &str) -> Option<Vec<u8>> {
        self.get(column).flatten().map(cell_bytes)
    }

    /// A value the row must have.
    fn required(&self, column: &str) -> Result<Vec<u8>, String> {
        self.bytes(column)
            .ok_or_else(|| format!("the {column} cannot be empty (NULL)"))
    }
}

/// A TTL in seconds as `EXPIRE` takes it; `None` (no expiry) for NULL.
fn ttl(text: Option<&str>) -> Result<Option<u64>, String> {
    text.map(|t| {
        t.trim()
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("invalid ttl `{t}`: a number of seconds above 0, or NULL"))
    })
    .transpose()
}

fn score(text: Option<&str>) -> Result<String, String> {
    let text = text.unwrap_or("0").trim();
    text.parse::<f64>()
        .map(|_| text.to_owned())
        .map_err(|_| format!("invalid score `{text}`"))
}

fn ttl_command(key: &[u8], ttl: Option<u64>) -> String {
    match ttl {
        Some(n) => cmd!("EXPIRE", key, n.to_string().as_bytes()),
        None => cmd!("PERSIST", key),
    }
}

/// Text unique enough to mark the list items to remove, which `LREM` then takes out.
fn tombstone() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("__platypus_deleted_{}_{nanos}__", std::process::id())
}

/// The commands for `edits`.
pub fn script(edits: &Edits<'_>) -> Result<Script, String> {
    let select = cmd!("SELECT", edits.db.to_string());
    let mut out = Script {
        commands: vec![select.clone()],
        undo: vec![select],
    };
    let columns = edits.columns;
    // Deletes first, which frees hash fields and members for the renames that follow; on a
    // list they go after the updates, which find their items by index.
    let list = edits.kind == KeyType::List;
    if !list {
        deletes(edits, &mut out)?;
    }
    for update in &edits.updates {
        let old = Row::loaded(columns, &update.loaded);
        let new = Row::set(columns, &update.set);
        self::update(edits.kind, &old, &new, &mut out)?;
    }
    if list {
        deletes(edits, &mut out)?;
    }
    for insert in &edits.inserts {
        self::insert(edits.kind, &Row::set(columns, insert), &mut out)?;
    }
    Ok(out)
}

fn deletes(edits: &Edits<'_>, out: &mut Script) -> Result<(), String> {
    let columns = edits.columns;
    if edits.kind == KeyType::List {
        let mark = tombstone();
        let mut keys: Vec<Vec<u8>> = Vec::new();
        for loaded in &edits.deletes {
            let row = Row::loaded(columns, loaded);
            let key = row.required("key")?;
            let index = row.get("index").flatten().unwrap_or_default();
            out.commands
                .push(cmd!("LSET", &key, index.as_bytes(), mark.as_bytes()));
            out.undo.push(format!(
                "# The item {index} of {} was {}: put it back by hand",
                line(&[&key]),
                line(&[row.bytes("value").unwrap_or_default()])
            ));
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        for key in keys {
            out.commands.push(cmd!("LREM", &key, "0", mark.as_bytes()));
        }
        return Ok(());
    }
    for loaded in &edits.deletes {
        let row = Row::loaded(columns, loaded);
        let key = row.required("key")?;
        let key = key.as_slice();
        match edits.kind {
            KeyType::String | KeyType::Json => {
                out.commands.push(cmd!("DEL", key));
                let value = row.bytes("value").unwrap_or_default();
                out.undo.push(if edits.kind == KeyType::Json {
                    cmd!("JSON.SET", key, "$", &value)
                } else {
                    cmd!("SET", key, &value)
                });
                if let Some(n) = ttl(row.get("ttl").flatten())? {
                    out.undo.push(ttl_command(key, Some(n)));
                }
            }
            KeyType::Hash => {
                let field = row.required("field")?;
                let value = row.bytes("value").unwrap_or_default();
                out.commands.push(cmd!("HDEL", key, &field));
                out.undo.push(cmd!("HSET", key, &field, &value));
            }
            KeyType::Set => {
                let member = row.required("member")?;
                out.commands.push(cmd!("SREM", key, &member));
                out.undo.push(cmd!("SADD", key, &member));
            }
            KeyType::Zset => {
                let member = row.required("member")?;
                let score = score(row.get("score").flatten())?;
                out.commands.push(cmd!("ZREM", key, &member));
                out.undo.push(cmd!("ZADD", key, score.as_bytes(), &member));
            }
            KeyType::Stream => {
                let id = row.required("id")?;
                out.commands.push(cmd!("XDEL", key, &id));
                out.undo.push(format!(
                    "# The entry {} of {} cannot be put back",
                    line(&[&id]),
                    line(&[key])
                ));
            }
            KeyType::List => unreachable!("handled above"),
        }
    }
    Ok(())
}

/// The value and TTL of a string or JSON key.
fn update_value(
    kind: KeyType,
    key: &[u8],
    old: &Row<'_>,
    new: &Row<'_>,
    out: &mut Script,
) -> Result<(), String> {
    if new.get("value").is_some() {
        let value = new.required("value")?;
        let old_value = old.bytes("value").unwrap_or_default();
        if kind == KeyType::Json {
            out.commands.push(cmd!("JSON.SET", key, "$", &value));
            out.undo.push(cmd!("JSON.SET", key, "$", &old_value));
        } else {
            out.commands.push(cmd!("SET", key, &value, "KEEPTTL"));
            out.undo.push(cmd!("SET", key, &old_value, "KEEPTTL"));
        }
    }
    if new.get("ttl").is_some() {
        out.commands
            .push(ttl_command(key, ttl(new.get("ttl").flatten())?));
        out.undo
            .push(ttl_command(key, ttl(old.get("ttl").flatten())?));
    }
    Ok(())
}

/// A field of a hash: its value, or its name (the field moves).
fn update_field(key: &[u8], old: &Row<'_>, new: &Row<'_>, out: &mut Script) -> Result<(), String> {
    let field = old.required("field")?;
    let old_value = old.bytes("value").unwrap_or_default();
    let value = if new.get("value").is_some() {
        new.bytes("value").unwrap_or_default()
    } else {
        old_value.clone()
    };
    match new.get("field") {
        Some(Some(to)) if cell_bytes(to) != field => {
            let to = cell_bytes(to);
            out.commands.push(cmd!("HDEL", key, &field));
            out.commands.push(cmd!("HSETNX", key, &to, &value));
            out.undo.push(cmd!("HDEL", key, &to));
            out.undo.push(cmd!("HSET", key, &field, &old_value));
        }
        Some(None) => return Err("the field cannot be empty (NULL)".to_owned()),
        _ => {
            out.commands.push(cmd!("HSET", key, &field, &value));
            out.undo.push(cmd!("HSET", key, &field, &old_value));
        }
    }
    Ok(())
}

/// A member of a sorted set: its score, or its name (the member moves).
fn update_member(key: &[u8], old: &Row<'_>, new: &Row<'_>, out: &mut Script) -> Result<(), String> {
    let member = old.required("member")?;
    let old_score = score(old.get("score").flatten())?;
    let new_score = match new.get("score") {
        Some(s) => score(s)?,
        None => old_score.clone(),
    };
    match new.get("member") {
        Some(Some(to)) if cell_bytes(to) != member => {
            let to = cell_bytes(to);
            out.commands.push(cmd!("ZREM", key, &member));
            out.commands
                .push(cmd!("ZADD", key, "NX", new_score.as_bytes(), &to));
            out.undo.push(cmd!("ZREM", key, &to));
            out.undo
                .push(cmd!("ZADD", key, old_score.as_bytes(), &member));
        }
        Some(None) => return Err("the member cannot be empty (NULL)".to_owned()),
        _ => {
            out.commands
                .push(cmd!("ZADD", key, "XX", new_score.as_bytes(), &member));
            out.undo
                .push(cmd!("ZADD", key, "XX", old_score.as_bytes(), &member));
        }
    }
    Ok(())
}

fn update(kind: KeyType, old: &Row<'_>, new: &Row<'_>, out: &mut Script) -> Result<(), String> {
    let old_key = old.required("key")?;
    let renamed = match new.get("key") {
        Some(Some(text)) if cell_bytes(text) != old_key => Some(cell_bytes(text)),
        Some(None) => return Err("the key cannot be empty (NULL)".to_owned()),
        _ => None,
    };
    let key = match (&renamed, kind) {
        (None, _) => old_key.clone(),
        (Some(to), KeyType::String | KeyType::Json) => {
            // NX: a key that exists is not overwritten (the batch refuses it).
            out.commands.push(cmd!("RENAMENX", &old_key, to));
            to.clone()
        }
        (Some(_), _) => {
            return Err(format!(
                "the key of a {} row cannot change: rename the key with RENAME in the editor",
                kind.table()
            ));
        }
    };
    let key = key.as_slice();
    let changed = |column: &str| new.get(column).is_some();
    match kind {
        KeyType::String | KeyType::Json => {
            update_value(kind, key, old, new, out)?;
            if renamed.is_some() {
                out.undo.push(cmd!("RENAME", key, &old_key));
            }
        }
        KeyType::Hash => update_field(key, old, new, out)?,
        KeyType::List => {
            if changed("index") && new.get("index") != old.get("index") {
                return Err("the index of a list item cannot change".to_owned());
            }
            // Only the value of an item changes.
            if !changed("value") {
                return Ok(());
            }
            let index = old.get("index").flatten().unwrap_or_default();
            let value = new.bytes("value").unwrap_or_default();
            let old_value = old.bytes("value").unwrap_or_default();
            out.commands
                .push(cmd!("LSET", key, index.as_bytes(), &value));
            out.undo
                .push(cmd!("LSET", key, index.as_bytes(), &old_value));
        }
        KeyType::Set => {
            let member = old.required("member")?;
            let to = new.required("member")?;
            out.commands.push(cmd!("SREM", key, &member));
            out.commands.push(cmd!("SADD", key, &to));
            out.undo.push(cmd!("SREM", key, &to));
            out.undo.push(cmd!("SADD", key, &member));
        }
        KeyType::Zset => update_member(key, old, new, out)?,
        KeyType::Stream => {
            return Err("stream entries cannot change: add a new one with XADD".to_owned());
        }
    }
    Ok(())
}

fn insert(kind: KeyType, row: &Row<'_>, out: &mut Script) -> Result<(), String> {
    let key = row
        .bytes("key")
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "a new row needs a key".to_owned())?;
    let key = key.as_slice();
    match kind {
        KeyType::String | KeyType::Json => {
            let value = row.bytes("value").unwrap_or_default();
            // NX: a key that exists is not overwritten.
            if kind == KeyType::Json {
                out.commands.push(cmd!("JSON.SET", key, "$", &value, "NX"));
            } else {
                out.commands.push(cmd!("SET", key, &value, "NX"));
            }
            if let Some(n) = ttl(row.get("ttl").flatten())? {
                out.commands.push(ttl_command(key, Some(n)));
            }
            out.undo.push(cmd!("DEL", key));
        }
        KeyType::Hash => {
            let field = row.required("field")?;
            let value = row.bytes("value").unwrap_or_default();
            out.commands.push(cmd!("HSETNX", key, &field, &value));
            out.undo.push(cmd!("HDEL", key, &field));
        }
        KeyType::List => {
            // New items go at the end, whatever index they were given.
            let value = row.bytes("value").unwrap_or_default();
            out.commands.push(cmd!("RPUSH", key, &value));
            out.undo.push(cmd!("LREM", key, "-1", &value));
        }
        KeyType::Set => {
            let member = row.required("member")?;
            out.commands.push(cmd!("SADD", key, &member));
            out.undo.push(cmd!("SREM", key, &member));
        }
        KeyType::Zset => {
            let member = row.required("member")?;
            let score = score(row.get("score").flatten())?;
            out.commands
                .push(cmd!("ZADD", key, "NX", score.as_bytes(), &member));
            out.undo.push(cmd!("ZREM", key, &member));
        }
        KeyType::Stream => {
            let fields = row.get("fields").flatten().unwrap_or("{}");
            let object: serde_json::Map<String, serde_json::Value> = serde_json::from_str(fields)
                .map_err(|_| {
                format!("the fields of a stream entry are a JSON object: {fields}")
            })?;
            if object.is_empty() {
                return Err("a stream entry needs at least one field".to_owned());
            }
            let id = row
                .get("id")
                .flatten()
                .filter(|id| !id.trim().is_empty())
                .unwrap_or("*");
            let mut args: Vec<Vec<u8>> =
                vec![b"XADD".to_vec(), key.to_vec(), id.as_bytes().to_vec()];
            for (name, value) in object {
                args.push(name.into_bytes());
                args.push(match value {
                    serde_json::Value::String(s) => s.into_bytes(),
                    other => other.to_string().into_bytes(),
                });
            }
            out.commands.push(line(&args));
            out.undo.push(format!(
                "# Delete the new entry of {} with XDEL",
                line(&[key])
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|&n| n.to_owned()).collect()
    }

    fn some(values: &[&str]) -> Vec<Option<String>> {
        values.iter().map(|v| Some((*v).to_owned())).collect()
    }

    fn edits(kind: KeyType, columns: &[String]) -> Edits<'_> {
        Edits {
            db: 2,
            kind,
            columns,
            updates: Vec::new(),
            inserts: Vec::new(),
            deletes: Vec::new(),
        }
    }

    #[test]
    fn strings_update_rename_expire_and_delete() {
        let columns = cols(&["key", "value", "ttl"]);
        let mut e = edits(KeyType::String, &columns);
        e.updates.push(Update {
            loaded: vec![Some("a".into()), Some("1".into()), None],
            set: vec![
                (0, Some("b".into())),
                (1, Some("two words".into())),
                (2, Some("60".into())),
            ],
        });
        e.deletes.push(some(&["gone", "x", "5"]));
        e.inserts.push(vec![
            (0, Some("new".into())),
            (1, Some("v".into())),
            (2, Some("10".into())),
        ]);
        let s = script(&e).unwrap();
        assert_eq!(
            s.commands,
            [
                "SELECT 2",
                "DEL gone",
                "RENAMENX a b",
                "SET b \"two words\" KEEPTTL",
                "EXPIRE b 60",
                "SET new v NX",
                "EXPIRE new 10",
            ]
        );
        assert_eq!(
            s.undo,
            [
                "SELECT 2",
                "SET gone x",
                "EXPIRE gone 5",
                "SET b 1 KEEPTTL",
                "PERSIST b",
                "RENAME b a",
                "DEL new",
            ]
        );
    }

    #[test]
    fn hashes_rename_fields_and_refuse_key_changes() {
        let columns = cols(&["key", "field", "value"]);
        let mut e = edits(KeyType::Hash, &columns);
        e.updates.push(Update {
            loaded: some(&["h", "f", "v"]),
            set: vec![(1, Some("g".into()))],
        });
        let s = script(&e).unwrap();
        assert_eq!(s.commands[1..], ["HDEL h f", "HSETNX h g v"]);
        e.updates[0].set = vec![(0, Some("other".into()))];
        assert!(script(&e).unwrap_err().contains("RENAME"));
        e.updates[0].set = vec![(2, None)];
        assert_eq!(script(&e).unwrap().commands[1], "HSET h f \"\"");
    }

    #[test]
    fn lists_mark_and_remove_deleted_items_after_updates() {
        let columns = cols(&["key", "index", "value"]);
        let mut e = edits(KeyType::List, &columns);
        e.deletes.push(some(&["l", "0", "a"]));
        e.deletes.push(some(&["l", "2", "c"]));
        e.updates.push(Update {
            loaded: some(&["l", "1", "b"]),
            set: vec![(2, Some("B".into()))],
        });
        e.inserts
            .push(vec![(0, Some("l".into())), (2, Some("z".into()))]);
        let s = script(&e).unwrap();
        assert_eq!(s.commands[1], "LSET l 1 B");
        assert!(s.commands[2].starts_with("LSET l 0 __platypus_deleted_"));
        assert!(s.commands[4].starts_with("LREM l 0 __platypus_deleted_"));
        assert_eq!(s.commands[5], "RPUSH l z");
        assert_eq!(s.commands.len(), 6);
        // An index typed again as it was changes nothing: the item keeps its value.
        let mut e = edits(KeyType::List, &columns);
        e.updates.push(Update {
            loaded: some(&["l", "1", "b"]),
            set: vec![(1, Some("1".into()))],
        });
        assert_eq!(script(&e).unwrap().commands, ["SELECT 2"]);
    }

    #[test]
    fn zsets_and_sets() {
        let columns = cols(&["key", "member", "score"]);
        let mut e = edits(KeyType::Zset, &columns);
        e.updates.push(Update {
            loaded: some(&["z", "m", "1.5"]),
            set: vec![(2, Some("3".into()))],
        });
        e.inserts
            .push(vec![(0, Some("z".into())), (1, Some("n".into()))]);
        let s = script(&e).unwrap();
        assert_eq!(s.commands[1..], ["ZADD z XX 3 m", "ZADD z NX 0 n"]);
        e.updates[0].set = vec![(2, Some("x".into()))];
        assert!(script(&e).is_err());

        let columns = cols(&["key", "member"]);
        let mut e = edits(KeyType::Set, &columns);
        e.updates.push(Update {
            loaded: some(&["s", "a"]),
            set: vec![(1, Some("b".into()))],
        });
        assert_eq!(script(&e).unwrap().commands[1..], ["SREM s a", "SADD s b"]);
    }

    #[test]
    fn binary_values_and_bad_input() {
        let columns = cols(&["key", "value", "ttl"]);
        let mut e = edits(KeyType::String, &columns);
        e.updates.push(Update {
            loaded: vec![Some(r#""\xff""#.into()), Some("x".into()), None],
            set: vec![(1, Some("y".into()))],
        });
        assert_eq!(script(&e).unwrap().commands[1], r#"SET "\xff" y KEEPTTL"#);
        e.updates[0].set = vec![(2, Some("-3".into()))];
        assert!(script(&e).is_err());
        e.updates.clear();
        e.inserts.push(vec![(1, Some("v".into()))]);
        assert!(script(&e).unwrap_err().contains("key"));
    }

    #[test]
    fn json_and_streams() {
        let columns = cols(&["key", "value", "ttl"]);
        let mut e = edits(KeyType::Json, &columns);
        e.inserts.push(vec![
            (0, Some("doc".into())),
            (1, Some(r#"{"a":1}"#.into())),
        ]);
        assert_eq!(
            script(&e).unwrap().commands[1],
            r#"JSON.SET doc $ "{\"a\":1}" NX"#
        );
        let columns = cols(&["key", "id", "fields"]);
        let mut e = edits(KeyType::Stream, &columns);
        e.inserts.push(vec![
            (0, Some("s".into())),
            (2, Some(r#"{"f":"v","n":2}"#.into())),
        ]);
        e.deletes.push(some(&["s", "1-0", "{}"]));
        let s = script(&e).unwrap();
        assert_eq!(s.commands[1..], ["XDEL s 1-0", "XADD s * f v n 2"]);
        e.updates.push(Update {
            loaded: some(&["s", "1-0", "{}"]),
            set: vec![(2, Some("{}".into()))],
        });
        assert!(script(&e).is_err());
    }
}
