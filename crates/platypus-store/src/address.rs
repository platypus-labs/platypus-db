//! What the connection form's address fields get wrong when text is pasted into them: a path
//! in the quotes Explorer's "Copy as path" adds, SQL Server's `host,port`, a URL followed by
//! the punctuation of the sentence it was copied from, and the names of two connections.

use platypus_core::DriverKind;

use crate::SavedConnection;

/// A file path without the blanks and the quotes around it.
#[must_use]
pub fn clean_file_path(text: &str) -> &str {
    let text = text.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = text
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.trim();
        }
    }
    text
}

/// A URL without the `;` or `,` that follows it in the text it was copied from. Connection
/// strings (`Server=a;Database=b;`) end with `;` legitimately and are left as they are.
#[must_use]
pub fn trim_url_tail(text: &str) -> &str {
    let text = text.trim();
    if text.contains("://") {
        text.trim_end_matches([';', ',']).trim_end()
    } else {
        text
    }
}

/// Whether `text` holds a URL with a password (`postgres://user:secret@host`): one the file of
/// the connections would keep in clear.
#[must_use]
pub fn holds_url_password(text: &str) -> bool {
    text.match_indices("://").any(|(at, separator)| {
        let authority = text[at + separator.len()..]
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        authority
            .rsplit_once('@')
            .is_some_and(|(login, _)| login.contains(':'))
    })
}

/// SQL Server's `host,port` (SSMS, ADO.NET `Server=tcp:db,1433`) as the host and the port.
fn split_sqlserver_host(host: &str) -> Option<(&str, u16)> {
    let (name, port) = host.trim().rsplit_once(',')?;
    let port = port.trim().parse().ok().filter(|p| *p > 0)?;
    let name = name.trim();
    let name = name.strip_prefix("tcp:").unwrap_or(name).trim();
    (!name.is_empty()).then_some((name, port))
}

/// Puts the address fields of `connection` as the driver reads them: the file of an embedded
/// database without quotes or blanks, a SQL Server `host,port` split into its two fields.
pub fn tidy_address(connection: &mut SavedConnection) {
    if connection.driver.is_embedded() {
        connection.database = clean_file_path(&connection.database).to_owned();
    } else if connection.driver == DriverKind::SqlServer
        && let Some((host, port)) = split_sqlserver_host(&connection.host)
    {
        let (host, port) = (host.to_owned(), port);
        connection.host = host;
        connection.port = Some(port);
    }
}

/// Whether saving `connection` would give two connections one name (compared without case
/// or the blanks around): a new one with a taken name, or one renamed to a taken name. A
/// connection that already shares its name keeps it when it is edited.
#[must_use]
pub fn name_taken(saved: &[SavedConnection], connection: &SavedConnection) -> bool {
    let key = |name: &str| name.trim().to_lowercase();
    let name = key(&connection.name);
    if saved
        .iter()
        .any(|c| c.id == connection.id && key(&c.name) == name)
    {
        return false;
    }
    saved
        .iter()
        .any(|c| c.id != connection.id && key(&c.name) == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_with_a_password_is_found_wherever_it_is() {
        for text in [
            "postgres://u:secret@db/app",
            "postgres://localhost:5432/postgres://u:secret@db/app",
            "see mysql://u:p@ss:word@db:3306",
        ] {
            assert!(holds_url_password(text), "{text}");
        }
        for text in [
            "app",
            "postgres://u@db/app",
            "postgres://db:5432/app",
            "s3://bucket/a:b@c",
            "user:secret@db",
            r"C:\data\app.db",
            "",
        ] {
            assert!(!holds_url_password(text), "{text}");
        }
    }

    #[test]
    fn a_path_loses_its_quotes_and_blanks() {
        assert_eq!(clean_file_path(r#""D:\data\app.db""#), r"D:\data\app.db");
        assert_eq!(clean_file_path(r" D:\data\app.db "), r"D:\data\app.db");
        assert_eq!(clean_file_path(r#"  " D:\a b\x.db"  "#), r"D:\a b\x.db");
        assert_eq!(clean_file_path("'/tmp/x.db'"), "/tmp/x.db");
        assert_eq!(clean_file_path(r#""D:\data"#), r#""D:\data"#);
    }

    #[test]
    fn a_url_loses_the_punctuation_after_it() {
        assert_eq!(
            trim_url_tail("postgres://u:p@h:5433/shop;"),
            "postgres://u:p@h:5433/shop"
        );
        assert_eq!(
            trim_url_tail("postgres://u@h/shop, "),
            "postgres://u@h/shop"
        );
        assert_eq!(
            trim_url_tail("Server=a;Database=b;"),
            "Server=a;Database=b;"
        );
    }

    #[test]
    fn sql_server_host_and_port_are_split() {
        let mut c = SavedConnection::new("x", DriverKind::SqlServer);
        c.host = "tcp:db.example.com,51433".to_owned();
        tidy_address(&mut c);
        assert_eq!((c.host.as_str(), c.port), ("db.example.com", Some(51433)));
        let mut other = SavedConnection::new("x", DriverKind::Postgres);
        other.host = "a,1".to_owned();
        tidy_address(&mut other);
        assert_eq!(other.host, "a,1");
    }

    #[test]
    fn an_embedded_connections_file_is_cleaned() {
        let mut c = SavedConnection::new("x", DriverKind::DuckDb);
        c.database = r#" "D:\a.duckdb""#.to_owned();
        tidy_address(&mut c);
        assert_eq!(c.database, r"D:\a.duckdb");
    }

    #[test]
    fn two_connections_do_not_share_a_name() {
        let a = SavedConnection::new("QA lite", DriverKind::Sqlite);
        let saved = vec![a.clone()];
        assert!(!name_taken(&saved, &a));
        assert!(name_taken(
            &saved,
            &SavedConnection::new(" qa LITE ", DriverKind::Sqlite)
        ));
        assert!(!name_taken(
            &saved,
            &SavedConnection::new("other", DriverKind::Sqlite)
        ));
        let mut renamed = a;
        renamed.name = "Other".to_owned();
        assert!(!name_taken(&saved, &renamed));
    }
}
