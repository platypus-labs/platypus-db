//! The settings of DuckDB, from `duckdb_settings()`. The table has no default column, so the
//! defaults are the values of a fresh in-memory instance of the same library.

use std::collections::HashMap;

use platypus_core::{
    Connection, Error, Result, ServerParameter, TextRowExt, differs_from, query_text, set_value,
    sort_parameters,
};
use tokio::sync::oneshot;

use crate::error::map_error;

const SETTINGS: &str = "SELECT name, value, description, scope FROM duckdb_settings()";

pub(crate) async fn parameters(conn: &dyn Connection) -> Result<Vec<ServerParameter>> {
    let defaults = defaults().await.unwrap_or_default();
    let mut out: Vec<ServerParameter> = query_text(conn, SETTINGS)
        .await?
        .into_iter()
        .map(|r| {
            let name = r.text(0);
            let value = r.opt(1);
            let default = defaults.get(&name).cloned();
            ServerParameter {
                changed: differs_from(value.as_deref(), default.as_deref()),
                statement: value
                    .as_deref()
                    .map(|v| format!("SET {name} = {};", set_value(v, false))),
                context: r.opt(3),
                description: r.opt(2).filter(|d| !d.is_empty()),
                source: None,
                name,
                value,
                default,
                ..ServerParameter::default()
            }
        })
        .collect();
    sort_parameters(&mut out);
    Ok(out)
}

/// The values a new instance starts with. Read on a thread of its own: opening a database is
/// blocking work, and the library is loaded already, since a session is open.
async fn defaults() -> Result<HashMap<String, String>> {
    let (send, receive) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = send.send(fresh_settings());
    });
    receive
        .await
        .map_err(|_| Error::Other("reading the defaults was interrupted".to_owned()))?
}

fn fresh_settings() -> Result<HashMap<String, String>> {
    const SQL: &str = "SELECT name, value FROM duckdb_settings()";
    let fail = |e: duckdb::Error| map_error(&e, SQL);
    let db = duckdb::Connection::open_in_memory().map_err(fail)?;
    let mut statement = db.prepare(SQL).map_err(fail)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(fail)?;
    let mut out = HashMap::new();
    for row in rows {
        if let (name, Some(value)) = row.map_err(fail)? {
            out.insert(name, value);
        }
    }
    Ok(out)
}
