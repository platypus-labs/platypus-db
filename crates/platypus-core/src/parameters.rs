//! The configuration parameters of a database server, as it reports them, with the value it
//! starts from where it tells one. Read by [`Connection::server_parameters`], which the
//! admin tab lists with a filter for the ones that differ from their defaults.
//!
//! [`Connection::server_parameters`]: crate::Connection::server_parameters

use serde::Serialize;

/// One parameter of the server (a PostgreSQL setting, a MySQL variable, a SQL Server
/// configuration option, a SQLite pragma, …).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ServerParameter {
    pub name: String,
    /// The value in use, as the server writes it; `None` when it has none.
    pub value: Option<String>,
    /// What `value` counts in (`ms`, `8kB`, `MB`), where the server tells.
    pub unit: Option<String>,
    /// The value it has when nothing sets it, where the server tells.
    pub default: Option<String>,
    /// Whether `value` differs from the default. `false` when the server tells no default
    /// and no source either.
    pub changed: bool,
    /// Where the value comes from: `configuration file`, `COMPILED`, `command line`, …
    pub source: Option<String>,
    /// When a change takes effect: PostgreSQL's `context` (`postmaster` needs a restart,
    /// `sighup` a reload), or MariaDB's read-only flag.
    pub context: Option<String>,
    pub description: Option<String>,
    pub min: Option<String>,
    pub max: Option<String>,
    /// A change was made that waits for a restart to take effect.
    pub pending_restart: bool,
    /// A statement that sets it to its current value, to edit in the editor; `None` when
    /// the server does not allow setting it.
    pub statement: Option<String>,
}

/// Whether two values the server writes for a parameter are the same: ignoring case and
/// white space, `on`/`true`/`1` and `off`/`false`/`0` alike, and numbers by their value
/// (`1.0` and `1`).
#[must_use]
pub fn same_value(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    if let (Some(x), Some(y)) = (truth(a), truth(b)) {
        return x == y;
    }
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() <= f64::EPSILON * x.abs().max(y.abs()),
        _ => false,
    }
}

/// `value` read as a switch, if it is one of the words servers write for them.
fn truth(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Whether `value` differs from `default`; `false` when there is no default to compare.
#[must_use]
pub fn differs_from(value: Option<&str>, default: Option<&str>) -> bool {
    match (value, default) {
        (Some(v), Some(d)) => !same_value(v, d),
        _ => false,
    }
}

/// `value` as a string literal: quoted, with the quotes doubled, and the backslashes too for
/// the dialects (MySQL) that read them as escapes.
#[must_use]
pub fn string_literal(value: &str, backslash_escapes: bool) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' if backslash_escapes => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// `value` as the right-hand side of a `SET`: plain numbers and the words `ON`/`OFF` as they
/// are, anything else as a string literal.
#[must_use]
pub fn set_value(value: &str, backslash_escapes: bool) -> String {
    let plain = value.parse::<f64>().is_ok_and(f64::is_finite)
        && !value.starts_with('+')
        && !value.contains(['e', 'E', ' ']);
    if plain || matches!(value.to_ascii_uppercase().as_str(), "ON" | "OFF") {
        value.to_owned()
    } else {
        string_literal(value, backslash_escapes)
    }
}

/// Sorts `parameters` by name, the way a list reads them; two with the same name (a
/// ClickHouse session and server setting) by source.
pub fn sort_parameters(parameters: &mut [ServerParameter]) {
    parameters.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.source.cmp(&b.source)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_compare_as_the_server_means_them() {
        assert!(same_value("on", "ON"));
        assert!(same_value("1", "true"));
        assert!(same_value("off", "0"));
        assert!(same_value("1.0", "1"));
        assert!(same_value(" 128MB ", "128mb"));
        assert!(!same_value("on", "off"));
        assert!(!same_value("2", "true"));
        assert!(!same_value("abc", "abd"));
    }

    #[test]
    fn changed_needs_a_default_to_compare() {
        assert!(differs_from(Some("200"), Some("100")));
        assert!(!differs_from(Some("100"), Some("100.0")));
        assert!(!differs_from(Some("100"), None));
        assert!(!differs_from(None, Some("100")));
    }

    #[test]
    fn literals_escape_what_the_dialect_reads() {
        assert_eq!(string_literal("it's", false), "'it''s'");
        assert_eq!(string_literal(r"a\b", false), r"'a\b'");
        assert_eq!(string_literal(r"a\b'", true), r"'a\\b'''");
    }

    #[test]
    fn set_values_keep_numbers_and_switches_bare() {
        assert_eq!(set_value("128", false), "128");
        assert_eq!(set_value("-1", false), "-1");
        assert_eq!(set_value("0.5", false), "0.5");
        assert_eq!(set_value("on", false), "on");
        assert_eq!(set_value("1e3", false), "'1e3'");
        assert_eq!(set_value("128MB", false), "'128MB'");
        assert_eq!(set_value("", false), "''");
        assert_eq!(set_value("NaN", false), "'NaN'");
    }

    #[test]
    fn sorts_by_name() {
        let named = |name: &str| ServerParameter {
            name: name.to_owned(),
            ..ServerParameter::default()
        };
        let mut list = vec![named("b"), named("a")];
        sort_parameters(&mut list);
        assert_eq!(list[0].name, "a");
    }
}
