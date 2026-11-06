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
//!
//! ABI 2 (`api = 2`) adds the kinds `theme`, `hooks`, `tabs`, `panels` and `grid`, what each of
//! them declares ([`crate::extensions`]) and the `settings` of the plugin's page. Each of those
//! that reaches further than computing has a permission of its own, approved at install.
//! Whatever a manifest does not know (a field, a permission, a kind) is refused by name.

use serde::{Deserialize, Serialize};

use crate::extensions::{self, GridDecl, HookEvent, PanelDecl, SettingField, TabDecl, ThemeDecl};
use crate::package::{STATE_FILE, STATE_TMP};
use crate::{API_VERSION, MIN_API_VERSION, PluginError, settings};

/// The manifest's file name, at the root of a package.
pub const MANIFEST_FILE: &str = "plugin.toml";
/// The module's file name, beside the manifest.
pub const MODULE_FILE: &str = "plugin.wasm";
/// The files of the plugins' folder itself: a plugin named so would take their place.
const OWN_FILES: [&str; 3] = [STATE_FILE, STATE_TMP, settings::FILE];
/// The device names Windows keeps, with any case and any extension.
const DEVICE_NAMES: [&str; 4] = ["con", "prn", "aux", "nul"];

/// What a plugin provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Commands for the editor, the result grid or the navigator.
    Actions,
    /// A database driver, offered in the driver picker.
    Driver,
    /// Colours and fonts for the theme picker. A package with only this has no module.
    Theme,
    /// Runs on the events of its `hooks`.
    Hooks,
    /// The tab kinds of its `tabs`.
    Tabs,
    /// The side panels of its `panels`.
    Panels,
    /// The result grid's extensions of its `grid`.
    Grid,
}

/// What a permission opens, for what asks whether a plugin has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Runs on the app's events.
    Hooks,
    /// Shows its own documents in tabs and side panels.
    Tabs,
    /// Adds to the result grid.
    Grid,
}

/// What a plugin may do beyond computing: nothing unless listed, and only once the user
/// approved it at install.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one field per permission, as `plugin.toml` names them"
)]
pub struct Permissions {
    /// The hosts it may send HTTP requests to: names (`api.example.com`) or a wildcard for
    /// the subdomains of one (`*.example.com`). Empty: no HTTP at all.
    #[serde(default)]
    pub http: Vec<String>,
    /// An action may put text on the clipboard.
    #[serde(default)]
    pub clipboard: bool,
    /// Runs on the app's events: it sees that they happen and may stop a query or a save.
    #[serde(default)]
    pub hooks: bool,
    /// Shows its own documents in tabs and side panels.
    #[serde(default)]
    pub tabs: bool,
    /// Adds to the result grid.
    #[serde(default)]
    pub grid: bool,
}

impl Permissions {
    /// Asks for nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Opens `capability`.
    #[must_use]
    pub const fn allows(&self, capability: Capability) -> bool {
        match capability {
            Capability::Hooks => self.hooks,
            Capability::Tabs => self.tabs,
            Capability::Grid => self.grid,
        }
    }

    /// What these ask beyond `approved`: on an update, what the user has not said yes to.
    #[must_use]
    pub fn beyond(&self, approved: &Self) -> Self {
        Self {
            http: self
                .http
                .iter()
                .filter(|h| !approved.http.contains(h))
                .cloned()
                .collect(),
            clipboard: self.clipboard && !approved.clipboard,
            hooks: self.hooks && !approved.hooks,
            tabs: self.tabs && !approved.tabs,
            grid: self.grid && !approved.grid,
        }
    }
}

/// The contents of `plugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Unique and stable: lowercase letters, digits, `-`, `_` and `.`; the folder it is
    /// installed in, so a name that folder could not have on some system is refused
    /// ([`is_plain_name`]).
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// The guest ABI the module speaks, from [`MIN_API_VERSION`] to [`API_VERSION`].
    pub api: u32,
    pub kinds: Vec<Kind>,
    /// The events it hooks (kind `hooks`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<HookEvent>,
    #[serde(default)]
    pub permissions: Permissions,
    /// The tab kinds it adds (kind `tabs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<TabDecl>,
    /// The side panels it adds (kind `panels`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<PanelDecl>,
    /// What it adds to the result grid (kind `grid`).
    #[serde(default, skip_serializing_if = "GridDecl::is_empty")]
    pub grid: GridDecl,
    /// The fields of its settings page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingField>,
    /// Its theme (kind `theme`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeDecl>,
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
                "the id {:?} must be lowercase letters, digits, `-`, `_` or `.`, must not end in a dot, and must not be a device name of Windows or a file of the plugins' folder",
                self.id
            ));
        }
        if !is_plain_name(&self.version.to_ascii_lowercase()) {
            return invalid(format!("the version {:?} is not valid", self.version));
        }
        if self.name.trim().is_empty() {
            return invalid("the name is empty".to_owned());
        }
        if !(MIN_API_VERSION..=API_VERSION).contains(&self.api) {
            return invalid(format!(
                "it speaks plugin API {}, this version of the app speaks {MIN_API_VERSION} to {API_VERSION}",
                self.api
            ));
        }
        if self.kinds.is_empty() {
            return invalid("`kinds` lists nothing".to_owned());
        }
        if let Some(host) = self.permissions.http.iter().find(|h| !is_host_pattern(h)) {
            return invalid(format!(
                "{host:?} in `permissions.http` is not a host name such as api.example.com"
            ));
        }
        self.check_declared().or_else(invalid)
    }

    /// What the manifest declares: only under ABI 2, matching `kinds` and the permissions.
    fn check_declared(&self) -> Result<(), String> {
        let perms = &self.permissions;
        let abi2 = [
            ("the kind \"theme\"", self.provides(Kind::Theme)),
            ("the kind \"hooks\"", self.provides(Kind::Hooks)),
            ("the kind \"tabs\"", self.provides(Kind::Tabs)),
            ("the kind \"panels\"", self.provides(Kind::Panels)),
            ("the kind \"grid\"", self.provides(Kind::Grid)),
            ("`permissions.hooks`", perms.hooks),
            ("`permissions.tabs`", perms.tabs),
            ("`permissions.grid`", perms.grid),
            ("`settings`", !self.settings.is_empty()),
        ];
        if self.api < 2
            && let Some((what, _)) = abi2.iter().find(|(_, used)| *used)
        {
            return Err(format!("{what} needs `api = 2`"));
        }
        // A kind and what it declares go together, and need their permission (`panels` share
        // the one of `tabs`).
        let (hooks, tabs, panels, grid) = (
            !self.hooks.is_empty(),
            !self.tabs.is_empty(),
            !self.panels.is_empty(),
            !self.grid.is_empty(),
        );
        for (kind, name, declared, permission) in [
            (Kind::Hooks, "hooks", hooks, Some(("hooks", perms.hooks))),
            (Kind::Tabs, "tabs", tabs, Some(("tabs", perms.tabs))),
            (Kind::Panels, "panels", panels, Some(("tabs", perms.tabs))),
            (Kind::Grid, "grid", grid, Some(("grid", perms.grid))),
            (Kind::Theme, "theme", self.theme.is_some(), None),
        ] {
            if declared != self.provides(kind) {
                return Err(format!(
                    "`{name}` and the kind {name:?} go together: declare both or neither"
                ));
            }
            if let Some((needed, false)) = permission.filter(|_| declared) {
                return Err(format!("`{name}` needs `permissions.{needed}`"));
            }
        }
        for (asked, name, used) in [
            (perms.hooks, "hooks", hooks),
            (perms.tabs, "tabs", tabs || panels),
            (perms.grid, "grid", grid),
        ] {
            if asked && !used {
                return Err(format!(
                    "it asks for `permissions.{name}` and declares nothing that uses it"
                ));
            }
        }
        extensions::check_settings(&self.settings)?;
        extensions::check_documents(
            "tabs",
            self.tabs
                .iter()
                .map(|t| (t.kind.as_str(), t.title.as_str(), t.entry.as_str())),
        )?;
        extensions::check_documents(
            "panels",
            self.panels
                .iter()
                .map(|p| (p.id.as_str(), p.title.as_str(), p.entry.as_str())),
        )?;
        extensions::check_grid(&self.grid)?;
        self.theme.as_ref().map_or(Ok(()), extensions::check_theme)
    }

    /// Provides `kind`.
    #[must_use]
    pub fn provides(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }

    /// The HTML documents of its tabs and panels.
    pub fn documents(&self) -> impl Iterator<Item = &str> {
        self.tabs
            .iter()
            .map(|t| t.entry.as_str())
            .chain(self.panels.iter().map(|p| p.entry.as_str()))
    }

    /// Needs a WebAssembly module: everything does except a package that is only a theme.
    #[must_use]
    pub fn needs_module(&self) -> bool {
        self.kinds.iter().any(|k| *k != Kind::Theme)
    }
}

/// Safe as a single path component, and the same one on every system: no separators, no `..`,
/// nothing hidden, no trailing dot or space (Windows drops them, so `foo.` would be the folder of
/// `foo`), no device name of Windows, and none of the files beside the plugins' folders. A package
/// refused on one system is refused on all.
pub(crate) fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && !name.ends_with(['.', ' '])
        && !is_device_name(name)
        && !OWN_FILES.contains(&name)
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// `con`, `COM1`, `nul.txt`, `aux.tar.gz`: Windows opens the device whatever follows the first dot.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    let numbered = stem.len() == 4
        && (stem.starts_with("com") || stem.starts_with("lpt"))
        && stem.ends_with(|c: char| c.is_ascii_digit());
    numbered || DEVICE_NAMES.contains(&stem.as_str())
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
    fn a_name_is_one_folder_on_every_system() {
        for good in [
            "foo",
            "foo.bar",
            "a-b_c",
            "1.0.0",
            "1.0.0-rc1",
            "console",
            "com10",
            "plugins",
        ] {
            assert!(is_plain_name(good), "{good}");
        }
        for bad in [
            "",
            ".",
            "..",
            ".foo",
            "foo.",
            "foo..",
            "1.0.",
            "foo ",
            "a/b",
            "a\\b",
            "Foo",
            "con",
            "prn",
            "aux",
            "nul",
            "com1",
            "com9",
            "lpt1",
            "lpt9",
            "con.txt",
            "nul.tar.gz",
            "aux.",
            "lpt3.x",
            "plugins.json",
            "plugins.json.tmp",
            "settings.toml",
        ] {
            assert!(!is_plain_name(bad), "{bad:?}");
        }
        // A version is read in lower case: any case of these goes too.
        for (id, version) in [
            ("foo.", "1"),
            ("con", "1"),
            ("settings.toml", "1"),
            ("foo", "1.0."),
            ("foo", "CON"),
            ("foo", "Lpt2.x"),
            ("foo", "Plugins.json"),
        ] {
            let text = format!(
                "id = \"{id}\"\nname = \"N\"\nversion = \"{version}\"\napi = 1\nkinds = [\"actions\"]\n"
            );
            assert!(Manifest::parse(&text).is_err(), "{id} {version}");
        }
    }

    #[test]
    fn refuses_bad_manifests() {
        for (bad, why) in [
            (MINIMAL.replace("\"hello\"", "\"../x\""), "id"),
            (MINIMAL.replace("\"hello\"", "\"Hello\""), "id"),
            (MINIMAL.replace("api = 1", "api = 3"), "API"),
            (MINIMAL.replace("api = 1", "api = 0"), "API"),
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

    const ABI2: &str = r#"
        id = "full"
        name = "Full"
        version = "1.2.0"
        api = 2
        kinds = ["actions", "theme", "hooks", "tabs", "panels", "grid"]
        hooks = ["before_query", "after_query", "on_connect", "before_save"]

        [permissions]
        hooks = true
        tabs = true
        grid = true

        [[tabs]]
        kind = "notes"
        title = "Notes"
        entry = "ui/notes.html"

        [[panels]]
        id = "side"
        title = "Side"
        entry = "side.html"

        [[grid.columns]]
        id = "len"
        title = "Length"

        [[grid.renderers]]
        id = "json"
        title = "JSON tree"
        types = ["json", "jsonb"]

        [[grid.actions]]
        id = "open"
        title = "Open"

        [theme]
        name = "Sea"
        dark = { bg = "\#0b1d26" }
        fonts = { mono = "fonts/FiraCode.woff2" }

        [[settings]]
        key = "endpoint"
        label = "Endpoint"
        type = "text"
        default = "https://example.com"

        [[settings]]
        key = "retries"
        label = "Retries"
        type = "number"
        default = 3
        min = 0
        max = 10

        [[settings]]
        key = "verbose"
        label = "Verbose"
        type = "boolean"
        default = false

        [[settings]]
        key = "mode"
        label = "Mode"
        type = "choice"
        default = "fast"
        options = [{ value = "fast", label = "Fast" }, { value = "safe", label = "Safe" }]

        [[settings]]
        key = "token"
        label = "Token"
        description = "Kept in the keychain."
        type = "secret"
    "#;

    /// A valid hook plugin with `extra` at its end.
    fn hooks_manifest(extra: &str) -> String {
        format!(
            "id = \"h\"\nname = \"H\"\nversion = \"1\"\napi = 2\nkinds = [\"hooks\"]\nhooks = [\"before_query\"]\n[permissions]\nhooks = true\n{extra}"
        )
    }

    fn theme_manifest() -> &'static str {
        "id = \"t\"\nname = \"T\"\nversion = \"1\"\napi = 2\nkinds = [\"theme\"]\n[theme]\nname = \"T\"\nlight = { bg = \"#fff\" }\n"
    }

    #[test]
    fn reads_everything_abi_2_declares() {
        let m = Manifest::parse(&ABI2.replace("\\#", "#")).unwrap();
        assert_eq!(m.hooks.len(), 4);
        assert_eq!(m.tabs[0].entry, "ui/notes.html");
        assert_eq!(m.panels[0].id, "side");
        assert_eq!(m.grid.renderers[0].types, ["json", "jsonb"]);
        assert_eq!(m.settings.len(), 5);
        assert_eq!(m.settings[4].kind, extensions::SettingKind::Secret);
        assert!(m.permissions.allows(Capability::Hooks));
        assert!(m.permissions.allows(Capability::Tabs));
        assert!(m.permissions.allows(Capability::Grid));
        assert!(m.needs_module());
        // As an install writes it back, and reads it again.
        let again = Manifest::parse(&toml::to_string_pretty(&m).unwrap()).unwrap();
        assert_eq!(again, m);
    }

    #[test]
    fn a_theme_alone_needs_no_module() {
        let m = Manifest::parse(theme_manifest()).unwrap();
        assert!(!m.needs_module());
        assert!(Manifest::parse(MINIMAL).unwrap().needs_module());
    }

    #[test]
    fn abi_1_stays_as_it_was() {
        let m = Manifest::parse(MINIMAL).unwrap();
        let written: toml::Table = toml::to_string_pretty(&m).unwrap().parse().unwrap();
        // Nothing of ABI 2 is written for a plugin of ABI 1.
        for new in ["hooks", "tabs", "panels", "grid", "settings", "theme"] {
            assert!(!written.contains_key(new), "{new}: {written}");
        }
        // What was approved before these permissions existed still reads.
        let old: Permissions = serde_json::from_str(r#"{"http":[],"clipboard":true}"#).unwrap();
        assert!(old.clipboard && !old.hooks && !old.tabs && !old.grid);
        for (extra, named) in [
            ("hooks = [\"before_query\"]", "hooks"),
            ("[permissions]\nhooks = true", "permissions.hooks"),
            ("[permissions]\ntabs = true", "permissions.tabs"),
            ("[permissions]\ngrid = true", "permissions.grid"),
            (
                "[[settings]]\nkey = \"a\"\nlabel = \"A\"\ntype = \"text\"",
                "settings",
            ),
        ] {
            let e = Manifest::parse(&format!("{MINIMAL}\n{extra}"))
                .unwrap_err()
                .to_string();
            assert!(e.contains(named), "{named}: {e}");
        }
        for kind in ["theme", "hooks", "tabs", "panels", "grid"] {
            let e = Manifest::parse(&MINIMAL.replace("\"actions\"", &format!("\"{kind}\"")))
                .unwrap_err()
                .to_string();
            assert!(e.contains(kind) && e.contains("api = 2"), "{kind}: {e}");
        }
    }

    #[test]
    fn names_what_it_does_not_know() {
        for (extra, named) in [
            ("[permissions]\nhooks = true\nfiles = true", "files"),
            ("colour = \"red\"\n[permissions]\nhooks = true", "colour"),
            (
                "[[settings]]\nkey = \"a\"\nlabel = \"A\"\ntype = \"file\"",
                "file",
            ),
            (
                "[[settings]]\nkey = \"a\"\nlabel = \"A\"\ntype = \"text\"\nhidden = true",
                "hidden",
            ),
            (
                "[[tabs]]\nkind = \"a\"\ntitle = \"A\"\nentry = \"a.html\"\nicon = \"x\"",
                "icon",
            ),
            ("[grid]\nsort = []", "sort"),
            (
                "[theme]\nname = \"T\"\nlight = {}\nscript = \"x\"",
                "script",
            ),
        ] {
            let text = if extra.starts_with("colour") || extra.starts_with("[permissions]") {
                format!(
                    "id = \"h\"\nname = \"H\"\nversion = \"1\"\napi = 2\nkinds = [\"hooks\"]\nhooks = [\"before_query\"]\n{extra}"
                )
            } else {
                hooks_manifest(extra)
            };
            let e = Manifest::parse(&text).unwrap_err().to_string();
            assert!(e.contains(named), "{named}: {e}");
        }
        let e = Manifest::parse(&hooks_manifest("").replace("before_query", "on_shutdown"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("on_shutdown"), "{e}");
    }

    #[test]
    fn a_declaration_needs_its_kind_and_its_permission() {
        let perm = hooks_manifest("").replace("hooks = true", "");
        for (text, named) in [
            (perm, "permissions.hooks"),
            (
                hooks_manifest("").replace("hooks = [\"before_query\"]", ""),
                "go together",
            ),
            (
                hooks_manifest("").replace("kinds = [\"hooks\"]", "kinds = [\"actions\"]"),
                "go together",
            ),
            (
                format!("{MINIMAL}\n").replace("api = 1", "api = 2")
                    + "[permissions]\nhooks = true",
                "nothing that uses it",
            ),
            (
                MINIMAL
                    .replace("api = 1", "api = 2")
                    .replace("\"actions\"", "\"panels\"")
                    + "[[panels]]\nid = \"a\"\ntitle = \"A\"\nentry = \"a.html\"",
                "permissions.tabs",
            ),
            (
                MINIMAL
                    .replace("api = 1", "api = 2")
                    .replace("\"actions\"", "\"grid\"")
                    + "[[grid.actions]]\nid = \"a\"\ntitle = \"A\"",
                "permissions.grid",
            ),
            (
                theme_manifest().replace("[theme]\nname = \"T\"\nlight = { bg = \"#fff\" }\n", ""),
                "go together",
            ),
            (
                theme_manifest().replace("light = { bg = \"#fff\" }", ""),
                "colours",
            ),
        ] {
            let e = Manifest::parse(&text).unwrap_err().to_string();
            assert!(e.contains(named), "{named}: {e}");
        }
    }

    #[test]
    fn checks_the_fields_of_the_settings_page() {
        for (field, why) in [
            (
                "key = \"a.b\"\nlabel = \"A\"\ntype = \"text\"",
                "settings key",
            ),
            ("key = \"a\"\nlabel = \" \"\ntype = \"text\"", "label"),
            (
                "key = \"a\"\nlabel = \"A\"\ntype = \"secret\"\ndefault = \"x\"",
                "never written",
            ),
            ("key = \"a\"\nlabel = \"A\"\ntype = \"choice\"", "options"),
            (
                "key = \"a\"\nlabel = \"A\"\ntype = \"text\"\noptions = [{ value = \"x\", label = \"X\" }]",
                "options",
            ),
            (
                "key = \"a\"\nlabel = \"A\"\ntype = \"number\"\ndefault = \"x\"",
                "default",
            ),
            (
                "key = \"a\"\nlabel = \"A\"\ntype = \"number\"\ndefault = 5\nmax = 1",
                "out of range",
            ),
            (
                "key = \"a\"\nlabel = \"A\"\ntype = \"text\"\nmin = 1",
                "min",
            ),
            (
                "key = \"a\"\nlabel = \"A\"\ntype = \"choice\"\ndefault = \"z\"\noptions = [{ value = \"x\", label = \"X\" }]",
                "default",
            ),
        ] {
            let e = Manifest::parse(&hooks_manifest(&format!("[[settings]]\n{field}")))
                .unwrap_err()
                .to_string();
            assert!(e.contains(why), "{why}: {e}");
        }
        let twice = "[[settings]]\nkey = \"a\"\nlabel = \"A\"\ntype = \"text\"\n";
        let e = Manifest::parse(&hooks_manifest(&format!("{twice}{twice}")))
            .unwrap_err()
            .to_string();
        assert!(e.contains("two settings"), "{e}");
    }

    #[test]
    fn a_document_stays_inside_the_package() {
        for entry in [
            "../x.html",
            "/x.html",
            "a\\\\x.html",
            "x.js",
            "a//x.html",
            ".x/y.html",
        ] {
            let e = Manifest::parse(
                &(MINIMAL
                    .replace("api = 1", "api = 2")
                    .replace("\"actions\"", "\"tabs\"")
                    + &format!(
                        "[permissions]\ntabs = true\n[[tabs]]\nkind = \"a\"\ntitle = \"A\"\nentry = \"{entry}\""
                    )),
            )
            .unwrap_err()
            .to_string();
            assert!(e.contains("inside the package"), "{entry}: {e}");
        }
    }

    #[test]
    fn says_what_an_update_asks_beyond_what_was_approved() {
        let approved = Permissions {
            http: vec!["a.example.com".to_owned()],
            clipboard: true,
            ..Permissions::default()
        };
        let asked = Permissions {
            http: vec!["a.example.com".to_owned(), "b.example.com".to_owned()],
            clipboard: true,
            hooks: true,
            ..Permissions::default()
        };
        let beyond = asked.beyond(&approved);
        assert_eq!(beyond.http, ["b.example.com"]);
        assert!(beyond.hooks && !beyond.clipboard);
        assert!(approved.beyond(&approved).is_empty());
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
