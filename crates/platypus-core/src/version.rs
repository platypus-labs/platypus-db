//! Server versions as numbers, to send each server SQL it knows, and the oldest version of
//! each database that PlatypusDB is tested with.

use std::fmt;

use crate::ServerInfo;

/// A server version, e.g. `8.0.36`; compares by major, then minor, then patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServerVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ServerVersion {
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// The first `major[.minor[.patch]]` of a version string, as servers write it:
    /// `16.4 (Debian 16.4-1)`, `5.7.44-log`, `10.6.21-MariaDB-ubu2004`, `14.0.3465.1`,
    /// `PostgreSQL 12.22 on x86_64…`. `None` without digits.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let start = text.find(|c: char| c.is_ascii_digit())?;
        let mut parts = text[start..]
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .next()?
            .split('.')
            .map(|p| p.parse::<u32>().ok());
        let major = parts.next().flatten()?;
        let minor = parts.next().flatten().unwrap_or(0);
        let patch = parts.next().flatten().unwrap_or(0);
        Some(Self::new(major, minor, patch))
    }
}

impl fmt::Display for ServerVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)?;
        if self.patch > 0 {
            write!(f, ".{}", self.patch)?;
        }
        Ok(())
    }
}

/// The oldest version of a product that PlatypusDB is tested with, by the product name its
/// driver reports; `None` for products without one (SQLite, DuckDB, the PostgreSQL-compatible
/// servers, which version on their own, …).
#[must_use]
pub fn minimum_version(product: &str) -> Option<ServerVersion> {
    Some(match product {
        "PostgreSQL" => ServerVersion::new(12, 0, 0),
        "MySQL" => ServerVersion::new(5, 7, 0),
        "MariaDB" => ServerVersion::new(10, 6, 0),
        // 14.0 is SQL Server 2017.
        "SQL Server" => ServerVersion::new(14, 0, 0),
        "MongoDB" => ServerVersion::new(5, 0, 0),
        // `SCAN … TYPE`, ACL users and RESP3.
        "Redis" => ServerVersion::new(6, 0, 0),
        // The SQL endpoint, `_search` with PIT and the cancellable tasks API.
        "Elasticsearch" => ServerVersion::new(7, 10, 0),
        "OpenSearch" => ServerVersion::new(1, 0, 0),
        // `system_schema` (3.0) and protocol v4 (2.2); 4.0 is the oldest still maintained.
        "Cassandra" => ServerVersion::new(4, 0, 0),
        "ScyllaDB" => ServerVersion::new(5, 4, 0),
        _ => return None,
    })
}

impl ServerInfo {
    /// The version as numbers; `None` when the server wrote none.
    #[must_use]
    pub fn version_number(&self) -> Option<ServerVersion> {
        ServerVersion::parse(&self.version)
    }

    /// Whether the server is at least `version`; an unreadable version counts as recent, so
    /// that the newest SQL is tried.
    #[must_use]
    pub fn at_least(&self, version: ServerVersion) -> bool {
        self.version_number().is_none_or(|v| v >= version)
    }

    /// The oldest version tested, when this server is older than it.
    #[must_use]
    pub fn below_minimum(&self) -> Option<ServerVersion> {
        let minimum = minimum_version(&self.product)?;
        (!self.at_least(minimum)).then_some(minimum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(product: &str, version: &str) -> ServerInfo {
        ServerInfo {
            product: product.to_owned(),
            version: version.to_owned(),
            encrypted: false,
        }
    }

    #[test]
    fn reads_versions_as_servers_write_them() {
        for (text, expected) in [
            ("16.4 (Debian 16.4-1.pgdg120+2)", (16, 4, 0)),
            ("12.22", (12, 22, 0)),
            ("5.7.44-log", (5, 7, 44)),
            ("8.0.46", (8, 0, 46)),
            ("10.6.21-MariaDB-ubu2004", (10, 6, 21)),
            ("14.0.3465.1", (14, 0, 3465)),
            ("PostgreSQL 12.22 on x86_64-pc-linux-musl", (12, 22, 0)),
            ("v26.2.7", (26, 2, 7)),
            ("17", (17, 0, 0)),
        ] {
            let (major, minor, patch) = expected;
            assert_eq!(
                ServerVersion::parse(text),
                Some(ServerVersion::new(major, minor, patch)),
                "{text}"
            );
        }
        assert_eq!(ServerVersion::parse("unknown"), None);
        assert_eq!(ServerVersion::parse(""), None);
    }

    #[test]
    fn compares_and_prints() {
        assert!(ServerVersion::new(8, 0, 16) > ServerVersion::new(5, 7, 44));
        assert!(ServerVersion::new(10, 11, 0) > ServerVersion::new(10, 6, 21));
        assert_eq!(ServerVersion::new(8, 0, 0).to_string(), "8.0");
        assert_eq!(ServerVersion::new(8, 0, 16).to_string(), "8.0.16");
    }

    #[test]
    fn says_when_a_server_is_older_than_tested() {
        assert_eq!(
            info("PostgreSQL", "11.22").below_minimum(),
            Some(ServerVersion::new(12, 0, 0))
        );
        assert_eq!(info("PostgreSQL", "12.0").below_minimum(), None);
        assert_eq!(
            info("MySQL", "5.6.51").below_minimum(),
            Some(ServerVersion::new(5, 7, 0))
        );
        assert_eq!(info("MariaDB", "10.6.21-MariaDB").below_minimum(), None);
        assert!(info("SQL Server", "13.0.5026.0").below_minimum().is_some());
        assert_eq!(info("SQLite", "3.0.0").below_minimum(), None);
        assert_eq!(
            info("PostgreSQL", "devel").below_minimum(),
            None,
            "unreadable"
        );
        assert!(info("MySQL", "8.0.36").at_least(ServerVersion::new(8, 0, 16)));
        assert!(!info("MySQL", "5.7.44").at_least(ServerVersion::new(8, 0, 16)));
    }
}
