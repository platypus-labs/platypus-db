//! What runs right after a connection opens: the default schema, the time zone and the
//! user's bootstrap statements. Drivers run it at the end of `connect`, so a reconnect sets
//! the session up again.

use futures::TryStreamExt;

use crate::{Connection, DriverKind, Error, ExecOptions, ExecResult, Result};

/// Session settings applied after connecting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionInit {
    /// PostgreSQL's and DuckDB's `search_path`, MySQL's current database; SQLite has none.
    pub schema: Option<String>,
    /// An IANA name such as `Europe/Rome`, or an offset such as `+02:00`; SQLite has none.
    pub time_zone: Option<String>,
    /// Run in order, after the settings above.
    pub statements: Vec<String>,
}

impl SessionInit {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.schema.is_none() && self.time_zone.is_none() && self.statements.is_empty()
    }

    /// The statements that set the session up on `driver`.
    pub fn sql(&self, driver: DriverKind) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if let Some(schema) = self.schema.as_deref().filter(|s| !s.is_empty()) {
            match driver {
                DriverKind::Postgres => {
                    out.push(format!("SET search_path TO {}", quote(schema, '"')));
                }
                DriverKind::MySql => out.push(format!("USE {}", quote(schema, '`'))),
                // `USE` holds for the HTTP session the driver keeps.
                DriverKind::ClickHouse => out.push(format!("USE {}", quote_clickhouse(schema))),
                // DuckDB takes the search path as a string of identifiers.
                DriverKind::DuckDb => out.push(format!(
                    "SET search_path = '{}'",
                    quote(schema, '"').replace('\'', "''")
                )),
                DriverKind::Sqlite | DriverKind::Libsql => {
                    return Err(Error::Config("SQLite has no default schema".to_owned()));
                }
                // The default schema belongs to the login's database user, not the session.
                DriverKind::SqlServer => {
                    return Err(Error::Config(
                        "SQL Server sets the default schema per user, not per session".to_owned(),
                    ));
                }
                DriverKind::Mongo => out.push(format!("use {schema}")),
                // Their schema is a field of the connection, passed to the driver.
                DriverKind::Snowflake
                | DriverKind::BigQuery
                | DriverKind::Databricks
                | DriverKind::Trino
                | DriverKind::Adbc => {
                    return Err(Error::Config(
                        "set the schema in the connection's own fields".to_owned(),
                    ));
                }
            }
        }
        if let Some(zone) = self.time_zone.as_deref().filter(|z| !z.is_empty()) {
            // MySQL reads backslashes in strings as escapes; no zone name has one.
            if zone.contains(['\\', '\'']) {
                return Err(Error::Config(format!("invalid time zone `{zone}`")));
            }
            match driver {
                DriverKind::Postgres => out.push(format!("SET TIME ZONE '{zone}'")),
                DriverKind::MySql => out.push(format!("SET time_zone = '{zone}'")),
                DriverKind::ClickHouse => out.push(format!("SET session_timezone = '{zone}'")),
                // Time zones come with DuckDB's ICU extension, which the app does not bundle.
                DriverKind::DuckDb => {
                    return Err(Error::Config(
                        "DuckDB has no session time zone here".to_owned(),
                    ));
                }
                DriverKind::Sqlite | DriverKind::Libsql => {
                    return Err(Error::Config("SQLite has no session time zone".to_owned()));
                }
                DriverKind::SqlServer => {
                    return Err(Error::Config(
                        "SQL Server has no session time zone".to_owned(),
                    ));
                }
                DriverKind::Mongo => {
                    return Err(Error::Config("MongoDB has no session time zone".to_owned()));
                }
                DriverKind::Trino | DriverKind::Databricks => {
                    out.push(format!("SET TIME ZONE '{zone}'"));
                }
                DriverKind::Snowflake => {
                    out.push(format!("ALTER SESSION SET TIMEZONE = '{zone}'"));
                }
                DriverKind::BigQuery | DriverKind::Adbc => {
                    return Err(Error::Config(format!(
                        "{} has no session time zone here",
                        driver.name()
                    )));
                }
            }
        }
        out.extend(
            self.statements
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        );
        Ok(out)
    }
}

/// `name` as a quoted identifier, the quote character doubled inside.
fn quote(name: &str, q: char) -> String {
    let doubled: String = [q, q].iter().collect();
    format!("{q}{}{q}", name.replace(q, &doubled))
}

/// A ClickHouse identifier: backticks, with backslash escapes inside.
fn quote_clickhouse(name: &str) -> String {
    format!("`{}`", name.replace('\\', "\\\\").replace('`', "\\`"))
}

/// Runs the session setup on a new connection; a failing statement is named in the error.
pub async fn run_init(conn: &dyn Connection, init: &SessionInit, driver: DriverKind) -> Result<()> {
    for sql in init.sql(driver)? {
        let result = match conn.execute(&sql, &ExecOptions::default()).await {
            Ok(ExecResult::Rows(stream)) => stream.try_for_each(|_| async { Ok(()) }).await,
            Ok(ExecResult::Affected { .. }) => Ok(()),
            Err(e) => Err(e),
        };
        result.map_err(|e| Error::Connect(format!("session setup failed at `{sql}`: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_by_driver() {
        let init = SessionInit {
            schema: Some("sales \"eu\"".to_owned()),
            time_zone: Some("Europe/Rome".to_owned()),
            statements: vec!["SET x = 1".to_owned(), "  ".to_owned()],
        };
        assert_eq!(
            init.sql(DriverKind::Postgres).unwrap(),
            [
                "SET search_path TO \"sales \"\"eu\"\"\"",
                "SET TIME ZONE 'Europe/Rome'",
                "SET x = 1"
            ]
        );
        assert_eq!(
            init.sql(DriverKind::MySql).unwrap(),
            [
                "USE `sales \"eu\"`",
                "SET time_zone = 'Europe/Rome'",
                "SET x = 1"
            ]
        );
        assert_eq!(
            init.sql(DriverKind::ClickHouse).unwrap(),
            [
                "USE `sales \"eu\"`",
                "SET session_timezone = 'Europe/Rome'",
                "SET x = 1"
            ]
        );
        assert_eq!(quote_clickhouse("a`b\\c"), "`a\\`b\\\\c`");
        assert!(init.sql(DriverKind::DuckDb).is_err());
        let schema_only = SessionInit {
            schema: Some("sales".to_owned()),
            ..SessionInit::default()
        };
        assert_eq!(
            schema_only.sql(DriverKind::DuckDb).unwrap(),
            ["SET search_path = '\"sales\"'"]
        );
        assert!(init.sql(DriverKind::Sqlite).is_err());
        assert!(init.sql(DriverKind::SqlServer).is_err());
        let only = SessionInit {
            statements: vec!["PRAGMA foreign_keys = ON".to_owned()],
            ..SessionInit::default()
        };
        assert_eq!(
            only.sql(DriverKind::Sqlite).unwrap(),
            ["PRAGMA foreign_keys = ON"]
        );
        let bad = SessionInit {
            time_zone: Some("x'; DROP TABLE t; --".to_owned()),
            ..SessionInit::default()
        };
        assert!(bad.sql(DriverKind::Postgres).is_err());
    }
}
