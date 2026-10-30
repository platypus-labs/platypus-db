//! Connections as they leave the machine: an allow-list. [`ShareConnection`] has a field for
//! each thing that may be written to a sync file or a shared folder and none for anything that
//! could hold a credential, so a new secret-bearing field added to `SavedConnection` cannot
//! leak by being forgotten here: it is dropped until someone adds it on purpose.
//!
//! Left out on purpose: the password (it is in the keychain anyway), `password_command` (a
//! shell command that would run on a teammate's machine), `.pgpass`, the SSL certificate and
//! key paths, the SSH key paths and the ids of the keychain entries, and the driver options
//! that hold a credential or a path on this machine (see [`scrub_options`]).

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
/// holds one, and never leave the machine.
const SECRET_WORDS: [&str; 11] = [
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
];

/// Options that are a path on this machine: another computer has another path.
const LOCAL_PATH_OPTIONS: [&str; 2] = ["driver_file", "credentials_file"];

fn is_secret_name(name: &str) -> bool {
    let name = name.to_lowercase();
    SECRET_WORDS.iter().any(|w| name.contains(w))
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

/// A URL without the password of its `user:password@` part and without secret query pairs.
fn scrub_uri(uri: &str) -> String {
    let (head, query) = match uri.split_once('?') {
        Some((h, q)) => (h, Some(scrub_pairs(q, '&'))),
        None => (uri, None),
    };
    let head = match head.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
            let authority = match authority.rsplit_once('@') {
                // Neither the user nor the password: the user has its own field.
                Some((_, host)) => host,
                None => authority,
            };
            format!("{scheme}://{authority}{path}")
        }
        None => head.to_owned(),
    };
    match query {
        Some(q) if !q.is_empty() => format!("{head}?{q}"),
        _ => head,
    }
}

/// The driver options that may be shared, with credentials removed from the ones that stay.
#[must_use]
pub fn scrub_options(options: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    options
        .iter()
        .filter(|(name, _)| {
            !is_secret_name(name) && !LOCAL_PATH_OPTIONS.contains(&name.to_lowercase().as_str())
        })
        .map(|(name, value)| {
            let value = match name.to_lowercase().as_str() {
                "uri" | "url" => scrub_uri(value),
                "connection_string" | "dsn_string" => scrub_pairs(value, ';'),
                _ => value.clone(),
            };
            (name.clone(), value)
        })
        .collect()
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
        // The options the file cannot carry stay as this machine has them.
        // A hand-edited file may carry what this build would never write: not taken in.
        let mut options = scrub_options(&self.options);
        for (name, value) in existing.map(|e| &e.options).into_iter().flatten() {
            if is_secret_name(name) || LOCAL_PATH_OPTIONS.contains(&name.to_lowercase().as_str()) {
                options.insert(name.clone(), value.clone());
            }
        }
        c.options = options;
        c.virtual_keys = self.virtual_keys;
        if existing.is_none() {
            c.save_password = !c.driver.is_embedded() && !c.signs_in_without_password();
        }
        c
    }
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
