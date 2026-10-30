//! `plugin.toml`: who a plugin is, what it provides and the permissions it asks for.
//!
//! ```toml
//! id = "uppercase-keywords"
//! name = "Uppercase SQL keywords"
//! version = "1.0.0"
//! author = "Jane Doe"
//! description = "Writes the SQL keywords of the editor in capitals."
//! api = 1
//! kinds = ["actions"]
//!
//! [permissions]
//! http = ["api.example.com", "*.example.org"]
//! clipboard = true
//! ```

use serde::{Deserialize, Serialize};

use crate::{API_VERSION, PluginError};

/// The manifest's file name, at the root of a package.
pub const MANIFEST_FILE: &str = "plugin.toml";
/// The module's file name, beside the manifest.
pub const MODULE_FILE: &str = "plugin.wasm";

/// What a plugin provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Commands for the editor, the result grid or the navigator.
    Actions,
    /// A database driver, offered in the driver picker.
    Driver,
}

/// What a plugin may do beyond computing: nothing unless listed, and only once the user
/// approved it at install.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    /// The hosts it may send HTTP requests to: names (`api.example.com`) or a wildcard for
    /// the subdomains of one (`*.example.com`). Empty: no HTTP at all.
    #[serde(default)]
    pub http: Vec<String>,
    /// An action may put text on the clipboard.
    #[serde(default)]
    pub clipboard: bool,
}

impl Permissions {
    /// Asks for nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.http.is_empty() && !self.clipboard
    }
}

/// The contents of `plugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Unique and stable: lowercase letters, digits, `-`, `_` and `.`; the folder it is
    /// installed in.
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// The guest ABI the module speaks: [`API_VERSION`].
    pub api: u32,
    pub kinds: Vec<Kind>,
    #[serde(default)]
    pub permissions: Permissions,
}

impl Manifest {
    /// Reads and checks a manifest.
    pub fn parse(text: &str) -> Result<Self, PluginError> {
        let manifest: Self = toml::from_str(text)
            .map_err(|e| PluginError::Invalid(format!("{MANIFEST_FILE}: {e}")))?;
        manifest.check()?;
        Ok(manifest)
    }

    fn check(&self) -> Result<(), PluginError> {
        let invalid = |why: String| Err(PluginError::Invalid(format!("{MANIFEST_FILE}: {why}")));
        if !is_plain_name(&self.id) {
            return invalid(format!(
                "the id {:?} must be lowercase letters, digits, `-`, `_` or `.`",
                self.id
            ));
        }
        if !is_plain_name(&self.version.to_ascii_lowercase()) {
            return invalid(format!("the version {:?} is not valid", self.version));
        }
        if self.name.trim().is_empty() {
            return invalid("the name is empty".to_owned());
        }
        if self.api != API_VERSION {
            return invalid(format!(
                "it speaks plugin API {}, this version of the app speaks {API_VERSION}",
                self.api
            ));
        }
        if self.kinds.is_empty() {
            return invalid("`kinds` lists neither \"actions\" nor \"driver\"".to_owned());
        }
        if let Some(host) = self.permissions.http.iter().find(|h| !is_host_pattern(h)) {
            return invalid(format!(
                "{host:?} in `permissions.http` is not a host name such as api.example.com"
            ));
        }
        Ok(())
    }

    /// Provides `kind`.
    #[must_use]
    pub fn provides(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }
}

/// Safe as a single path component: no separators, no `..`, nothing hidden.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// A host name, or `*.` and one: letters, digits, `-` and `.`, no scheme, port or path.
fn is_host_pattern(pattern: &str) -> bool {
    let host = pattern.strip_prefix("*.").unwrap_or(pattern);
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
}

/// `host` is allowed by one of `patterns`: equal to a name (ignoring case), or a
/// subdomain of a `*.` one.
#[must_use]
pub fn host_allowed(patterns: &[String], host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    patterns.iter().any(|p| {
        let p = p.to_ascii_lowercase();
        match p.strip_prefix("*.") {
            Some(domain) => host
                .strip_suffix(domain)
                .is_some_and(|rest| rest.ends_with('.') && rest.len() > 1),
            None => host == p,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
        id = "hello"
        name = "Hello"
        version = "0.1.0"
        api = 1
        kinds = ["actions"]
    "#;

    #[test]
    fn reads_a_manifest_without_permissions() {
        let m = Manifest::parse(MINIMAL).unwrap();
        assert_eq!(m.id, "hello");
        assert!(m.provides(Kind::Actions));
        assert!(!m.provides(Kind::Driver));
        assert!(m.permissions.is_empty());
    }

    #[test]
    fn reads_permissions() {
        let m = Manifest::parse(&format!(
            "{MINIMAL}\n[permissions]\nhttp = [\"api.example.com\", \"*.example.org\"]\nclipboard = true\n"
        ))
        .unwrap();
        assert_eq!(m.permissions.http.len(), 2);
        assert!(m.permissions.clipboard);
    }

    #[test]
    fn refuses_bad_manifests() {
        for (bad, why) in [
            (MINIMAL.replace("\"hello\"", "\"../x\""), "id"),
            (MINIMAL.replace("\"hello\"", "\"Hello\""), "id"),
            (MINIMAL.replace("api = 1", "api = 2"), "API"),
            (MINIMAL.replace("[\"actions\"]", "[]"), "kinds"),
            (MINIMAL.replace("[\"actions\"]", "[\"shell\"]"), "kinds"),
            (format!("{MINIMAL}\nfs = true"), "fs"),
            (
                format!("{MINIMAL}\n[permissions]\nhttp = [\"https://x.com/a\"]"),
                "host",
            ),
            (format!("{MINIMAL}\n[permissions]\nfiles = true"), "files"),
        ] {
            let e = Manifest::parse(&bad).unwrap_err().to_string();
            assert!(e.contains(why), "{why}: {e}");
        }
    }

    #[test]
    fn matches_hosts_and_wildcards() {
        let allowed = vec!["api.example.com".to_owned(), "*.example.org".to_owned()];
        assert!(host_allowed(&allowed, "api.example.com"));
        assert!(host_allowed(&allowed, "API.Example.com."));
        assert!(!host_allowed(&allowed, "example.com"));
        assert!(!host_allowed(&allowed, "evil-api.example.com"));
        assert!(host_allowed(&allowed, "a.b.example.org"));
        assert!(!host_allowed(&allowed, "example.org"));
        assert!(!host_allowed(&allowed, "badexample.org"));
        assert!(!host_allowed(&[], "localhost"));
    }
}
