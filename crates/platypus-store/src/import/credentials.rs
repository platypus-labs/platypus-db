//! DBeaver's saved users and passwords: `.dbeaver/credentials-config.json` next to
//! `data-sources.json`. The file is AES-128-CBC with PKCS#7 padding under DBeaver's fixed,
//! public key; its first 16 bytes are the IV. Decrypted, it is JSON by connection id:
//! `{"<id>": {"#connection": {"user": …, "password": …}, "network/ssh_tunnel": {…}}}`.

use std::collections::BTreeMap;

use aes::Aes128;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit};
use platypus_core::Secret;
use serde_json::Value;

/// The key DBeaver encrypts the file with unless a master password protects it.
pub(super) const DEFAULT_KEY: [u8; 16] = [
    0xba, 0xbb, 0x4a, 0x9f, 0x77, 0x4a, 0xb8, 0x53, 0xc9, 0x6c, 0x2d, 0x65, 0x3d, 0xfe, 0x54, 0x4a,
];

/// A login: a user name and its password, either possibly missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Login {
    pub user: Option<String>,
    pub password: Option<Secret>,
}

/// What DBeaver saved for one connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Saved {
    pub connection: Login,
    /// The SSH tunnel's user and password, or the passphrase of its key.
    pub ssh: Login,
}

/// The saved logins of a project, by DBeaver's connection id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Credentials {
    /// The project has no `credentials-config.json`.
    Missing,
    /// It does not decrypt or parse: a master password, most likely.
    Unreadable,
    Read(BTreeMap<String, Saved>),
}

impl Credentials {
    /// Reads the file's bytes; `None` when there is no file.
    pub(super) fn of(bytes: Option<&[u8]>) -> Self {
        match bytes {
            None => Self::Missing,
            Some(bytes) => decrypt(bytes, &DEFAULT_KEY)
                .and_then(|plain| parse(&plain))
                .map_or(Self::Unreadable, Self::Read),
        }
    }

    /// What was saved for the connection with DBeaver's id `id`.
    pub(super) fn get(&self, id: &str) -> Option<&Saved> {
        match self {
            Self::Read(all) => all.get(id),
            Self::Missing | Self::Unreadable => None,
        }
    }
}

fn decrypt(bytes: &[u8], key: &[u8; 16]) -> Option<Vec<u8>> {
    if bytes.len() < 32 || !bytes.len().is_multiple_of(16) {
        return None;
    }
    let (iv, data) = bytes.split_at(16);
    cbc::Decryptor::<Aes128>::new_from_slices(key, iv)
        .ok()?
        .decrypt_padded_vec::<Pkcs7>(data)
        .ok()
}

fn parse(plain: &[u8]) -> Option<BTreeMap<String, Saved>> {
    let text = std::str::from_utf8(plain).ok()?;
    let root: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let entries = root.as_object()?;
    Some(
        entries
            .iter()
            .map(|(id, entry)| {
                let saved = Saved {
                    connection: login(entry.get("#connection"), "password"),
                    ssh: login(entry.get("network/ssh_tunnel"), "passphrase"),
                };
                (id.clone(), saved)
            })
            .collect(),
    )
}

/// The `user` and `password` of a part, or `fallback` for the password when it has none.
fn login(part: Option<&Value>, fallback: &str) -> Login {
    let field = |key: &str| {
        part.and_then(|p| p.get(key))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    Login {
        user: field("user"),
        password: field("password")
            .or_else(|| field(fallback))
            .map(Secret::new),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use cbc::cipher::BlockModeEncrypt;

    use super::*;

    /// `plain` encrypted as DBeaver writes it, under `key`.
    pub(in crate::import) fn encrypt(plain: &str, key: &[u8; 16]) -> Vec<u8> {
        let iv = [7u8; 16];
        let mut out = iv.to_vec();
        out.extend(
            cbc::Encryptor::<Aes128>::new_from_slices(key, &iv)
                .unwrap()
                .encrypt_padded_vec::<Pkcs7>(plain.as_bytes()),
        );
        out
    }

    const PLAIN: &str = r##"{"pg-1": {"#connection": {"user": "app", "password": "s3cret"},
        "network/ssh_tunnel": {"user": "ops", "password": "tunnel"}},
        "my-1": {"#connection": {"user": "root"}},
        "key-1": {"network/ssh_tunnel": {"passphrase": "phrase"}}}"##;

    #[test]
    fn round_trip() {
        let creds = Credentials::of(Some(&encrypt(PLAIN, &DEFAULT_KEY)));
        let pg = creds.get("pg-1").unwrap();
        assert_eq!(pg.connection.user.as_deref(), Some("app"));
        assert_eq!(pg.connection.password, Some(Secret::new("s3cret")));
        assert_eq!(pg.ssh.user.as_deref(), Some("ops"));
        assert_eq!(pg.ssh.password, Some(Secret::new("tunnel")));
        let my = creds.get("my-1").unwrap();
        assert_eq!(
            (my.connection.user.as_deref(), &my.connection.password),
            (Some("root"), &None)
        );
        assert_eq!(my.ssh, Login::default());
        let key = creds.get("key-1").unwrap();
        assert_eq!(key.ssh.password, Some(Secret::new("phrase")));
        assert_eq!(creds.get("other"), None);
    }

    #[test]
    fn wrong_key_or_garbage_is_unreadable() {
        let other = Credentials::of(Some(&encrypt(PLAIN, &[1; 16])));
        assert_eq!(other, Credentials::Unreadable);
        assert_eq!(
            Credentials::of(Some(b"\x01\x02encrypted")),
            Credentials::Unreadable
        );
        // Decrypts, but is not JSON.
        assert_eq!(
            Credentials::of(Some(&encrypt("not json", &DEFAULT_KEY))),
            Credentials::Unreadable
        );
        assert_eq!(Credentials::of(None), Credentials::Missing);
    }
}
