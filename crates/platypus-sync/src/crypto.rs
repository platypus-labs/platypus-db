//! The encrypted container: a versioned header, then the document sealed with AES-256-GCM
//! under a key derived from the sync passphrase with Argon2id, the same primitives as the
//! password vault. The whole header is authenticated, so changing any byte of the file, the
//! salt or the cost settings included, makes opening it fail.

use argon2::{Algorithm, Argon2, Params, Version};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

/// Identifies the file; the format version follows.
const MAGIC: &[u8; 8] = b"PLATYSYN";
/// The newest container format this build reads and writes.
pub const FORMAT_VERSION: u32 = 1;
const SALT_LEN: usize = 16;
const KEY_LEN: usize = 32;
const HEADER_LEN: usize = 8 + 4 + 12 + SALT_LEN + NONCE_LEN;
/// Upper bounds of the key settings read from a file: 1 GiB, 64 passes, 16 lanes.
const MAX_M_COST: u32 = 1 << 20;
const MAX_T_COST: u32 = 64;
const MAX_P_COST: u32 = 16;
const TAG_LEN: usize = 16;

/// What can go wrong reading or writing sync data.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("cannot access {path}: {message}")]
    Io { path: String, message: String },

    #[error("the sync passphrase cannot be empty")]
    EmptyPassphrase,

    #[error("this is not a PlatypusDB sync file")]
    NotSyncFile,

    #[error(
        "the sync file was written by a newer PlatypusDB (format {found}, this one reads up to {supported}): update the app on this computer"
    )]
    Newer { found: u32, supported: u32 },

    #[error("wrong sync passphrase, or the sync file is damaged")]
    WrongPassphrase,

    #[error("the sync file is damaged: {0}")]
    Damaged(String),

    #[error("{0}")]
    Invalid(String),
}

fn random<const N: usize>() -> Result<[u8; N], SyncError> {
    let mut out = [0; N];
    SystemRandom::new()
        .fill(&mut out)
        .map_err(|_| SyncError::Invalid("no random numbers from the system".to_owned()))?;
    Ok(out)
}

fn derive(
    passphrase: &str,
    salt: &[u8],
    (m, t, p): (u32, u32, u32),
) -> Result<[u8; KEY_LEN], SyncError> {
    let params = Params::new(m, t, p, Some(KEY_LEN))
        .map_err(|e| SyncError::Invalid(format!("sync key settings: {e}")))?;
    let mut key = [0; KEY_LEN];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| SyncError::Invalid(format!("sync key: {e}")))?;
    Ok(key)
}

fn sealing_key(key: &[u8; KEY_LEN]) -> LessSafeKey {
    // A 32-byte key is always valid for AES-256.
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).unwrap_or_else(|_| unreachable!()))
}

fn be(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Encrypts `plaintext` under `passphrase`: a fresh salt and nonce every time.
pub fn seal(passphrase: &str, plaintext: &[u8]) -> Result<Vec<u8>, SyncError> {
    seal_with(
        passphrase,
        plaintext,
        (
            Params::DEFAULT_M_COST,
            Params::DEFAULT_T_COST,
            Params::DEFAULT_P_COST,
        ),
    )
}

fn seal_with(
    passphrase: &str,
    plaintext: &[u8],
    cost: (u32, u32, u32),
) -> Result<Vec<u8>, SyncError> {
    if passphrase.is_empty() {
        return Err(SyncError::EmptyPassphrase);
    }
    let salt = random::<SALT_LEN>()?;
    let nonce = random::<NONCE_LEN>()?;
    let mut out = Vec::with_capacity(HEADER_LEN + plaintext.len() + TAG_LEN);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    for c in [cost.0, cost.1, cost.2] {
        out.extend_from_slice(&c.to_be_bytes());
    }
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    let mut key = derive(passphrase, &salt, cost)?;
    let header = out.clone();
    let mut body = plaintext.to_vec();
    let sealed = sealing_key(&key).seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(&header),
        &mut body,
    );
    key.fill(0);
    sealed.map_err(|_| SyncError::Invalid("cannot encrypt the sync data".to_owned()))?;
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decrypts a container. A file that is not one, one from a newer format and a wrong
/// passphrase (or any tampering) are told apart, and none of them returns data.
pub fn open(passphrase: &str, bytes: &[u8]) -> Result<Vec<u8>, SyncError> {
    if passphrase.is_empty() {
        return Err(SyncError::EmptyPassphrase);
    }
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(SyncError::NotSyncFile);
    }
    if bytes.len() < HEADER_LEN + TAG_LEN {
        return Err(SyncError::Damaged("the file is cut short".to_owned()));
    }
    let version = be(&bytes[8..12]);
    if version > FORMAT_VERSION {
        return Err(SyncError::Newer {
            found: version,
            supported: FORMAT_VERSION,
        });
    }
    let cost = (be(&bytes[12..16]), be(&bytes[16..20]), be(&bytes[20..24]));
    if cost.0 > MAX_M_COST || cost.1 > MAX_T_COST || cost.2 > MAX_P_COST {
        return Err(SyncError::Damaged(
            "the key settings are out of range".to_owned(),
        ));
    }
    let salt = &bytes[24..24 + SALT_LEN];
    let nonce: [u8; NONCE_LEN] = bytes[24 + SALT_LEN..HEADER_LEN]
        .try_into()
        .map_err(|_| SyncError::Damaged("invalid nonce".to_owned()))?;
    let mut key = derive(passphrase, salt, cost)
        .map_err(|_| SyncError::Damaged("invalid key settings".to_owned()))?;
    let mut body = bytes[HEADER_LEN..].to_vec();
    let opened = sealing_key(&key)
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(&bytes[..HEADER_LEN]),
            &mut body,
        )
        .map(|plain| plain.len());
    key.fill(0);
    let len = opened.map_err(|_| SyncError::WrongPassphrase)?;
    body.truncate(len);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap key settings: the tests seal many times.
    const FAST: (u32, u32, u32) = (8, 1, 1);

    #[test]
    fn round_trips_and_uses_fresh_salt_and_nonce() {
        let a = seal_with("correct horse", b"hello sync", FAST).unwrap();
        let b = seal_with("correct horse", b"hello sync", FAST).unwrap();
        assert_ne!(a, b);
        assert_eq!(open("correct horse", &a).unwrap(), b"hello sync");
        assert!(!a.windows(10).any(|w| w == b"hello sync"), "not in clear");
        assert_eq!(
            open(
                "correct horse",
                &seal_with("correct horse", b"", FAST).unwrap()
            )
            .unwrap(),
            b""
        );
    }

    #[test]
    fn the_default_settings_round_trip() {
        let sealed = seal("pass", b"data").unwrap();
        assert_eq!(open("pass", &sealed).unwrap(), b"data");
    }

    #[test]
    fn a_wrong_passphrase_is_an_error() {
        let sealed = seal_with("right", b"secret", FAST).unwrap();
        assert!(matches!(
            open("wrong", &sealed),
            Err(SyncError::WrongPassphrase)
        ));
        assert!(matches!(open("", &sealed), Err(SyncError::EmptyPassphrase)));
        assert!(matches!(seal("", b"x"), Err(SyncError::EmptyPassphrase)));
    }

    #[test]
    fn every_tampered_byte_fails() {
        let sealed = seal_with("p", b"some data to protect", FAST).unwrap();
        for i in 0..sealed.len() {
            let mut bad = sealed.clone();
            bad[i] ^= 1;
            assert!(open("p", &bad).is_err(), "byte {i} changed");
        }
        for len in 0..sealed.len() {
            assert!(open("p", &sealed[..len]).is_err(), "cut at {len}");
        }
    }

    #[test]
    fn other_files_and_newer_formats_are_told_apart() {
        assert!(matches!(
            open("p", b"just text"),
            Err(SyncError::NotSyncFile)
        ));
        assert!(matches!(open("p", b""), Err(SyncError::NotSyncFile)));
        let mut newer = seal_with("p", b"x", FAST).unwrap();
        newer[8..12].copy_from_slice(&(FORMAT_VERSION + 1).to_be_bytes());
        match open("p", &newer) {
            Err(SyncError::Newer { found, supported }) => {
                assert_eq!((found, supported), (FORMAT_VERSION + 1, FORMAT_VERSION));
            }
            other => panic!("{other:?}"),
        }
        let mut huge = seal_with("p", b"x", FAST).unwrap();
        huge[12..16].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(open("p", &huge), Err(SyncError::Damaged(_))));
    }
}
