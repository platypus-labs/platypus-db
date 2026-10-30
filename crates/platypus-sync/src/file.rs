//! The sync file in the folder the user picked: `platypus-sync.bin`, the document sealed with
//! the sync passphrase. Writes go to a temporary file renamed over the old one, so a crash or
//! a full disk never leaves a half-written file for the other machines to read.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

use crate::Result;
use crate::crypto::{self, SyncError};
use crate::model::{DOCUMENT_FORMAT, Document};

/// The file's name in the sync folder.
pub const FILE_NAME: &str = "platypus-sync.bin";

fn io(path: &Path, e: &std::io::Error) -> SyncError {
    SyncError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    }
}

/// Writes `bytes` to `path` atomically.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| io(dir, &e))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| io(dir, &e))?;
    tmp.write_all(bytes).map_err(|e| io(path, &e))?;
    tmp.persist(path).map_err(|e| io(path, &e.error))?;
    Ok(())
}

fn missing_folder(folder: &Path) -> SyncError {
    SyncError::Io {
        path: folder.display().to_string(),
        message: "the sync folder was not found".to_owned(),
    }
}

/// A hash of the file's bytes, to tell whether another machine wrote it since it was read.
fn stamp_of(bytes: &[u8]) -> String {
    let hash = ring::digest::digest(&ring::digest::SHA256, bytes);
    hash.as_ref().iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The file's bytes, `None` when there is none.
fn read_bytes(folder: &Path) -> Result<Option<Vec<u8>>> {
    if !folder.is_dir() {
        return Err(missing_folder(folder));
    }
    let path = folder.join(FILE_NAME);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(&path, &e)),
    }
}

/// The stamp of the file in `folder` as it is now (see [`read_stamped`]); `None` when there
/// is no file. Cheap: nothing is decrypted.
pub fn stamp(folder: &Path) -> Result<Option<String>> {
    Ok(read_bytes(folder)?.as_deref().map(stamp_of))
}

/// The document in `folder`, or `None` when no sync file is there yet. A folder that does not
/// exist is an error (a drive that is not mounted must not look like an empty one).
pub fn read(folder: &Path, passphrase: &str) -> Result<Option<Document>> {
    Ok(read_stamped(folder, passphrase)?.0)
}

/// [`read`], with the stamp of the bytes read: the same stamp from [`stamp`] later means
/// nobody wrote the file in between.
pub fn read_stamped(folder: &Path, passphrase: &str) -> Result<(Option<Document>, Option<String>)> {
    let Some(bytes) = read_bytes(folder)? else {
        return Ok((None, None));
    };
    let plain = crypto::open(passphrase, &bytes)?;
    let document: Document =
        serde_json::from_slice(&plain).map_err(|e| SyncError::Damaged(e.to_string()))?;
    if document.format > DOCUMENT_FORMAT {
        return Err(SyncError::Newer {
            found: document.format,
            supported: DOCUMENT_FORMAT,
        });
    }
    Ok((Some(document), Some(stamp_of(&bytes))))
}

/// Seals `document` and writes it to `folder`; returns the stamp of what was written.
pub fn write(folder: &Path, passphrase: &str, document: &Document) -> Result<String> {
    if !folder.is_dir() {
        return Err(missing_folder(folder));
    }
    let plain = serde_json::to_vec(document).map_err(|e| SyncError::Invalid(e.to_string()))?;
    let sealed = crypto::seal(passphrase, &plain)?;
    write_atomic(&folder.join(FILE_NAME), &sealed)?;
    Ok(stamp_of(&sealed))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::model::Item;

    fn document() -> Document {
        Document {
            format: DOCUMENT_FORMAT,
            machine: "A".to_owned(),
            written: 1,
            items: BTreeMap::from([(
                "setting/x".to_owned(),
                Item::live(json!("CANARY_VALUE"), 1, "A"),
            )]),
        }
    }

    #[test]
    fn writes_reads_and_hides_the_content() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path(), "p").unwrap(), None);
        assert_eq!(stamp(dir.path()).unwrap(), None);
        write(dir.path(), "p", &document()).unwrap();
        let bytes = std::fs::read(dir.path().join(FILE_NAME)).unwrap();
        assert!(!bytes.windows(12).any(|w| w == b"CANARY_VALUE"));
        assert_eq!(read(dir.path(), "p").unwrap(), Some(document()));
        // The stamp follows the bytes: the same until someone writes again.
        let (_, read_stamp) = read_stamped(dir.path(), "p").unwrap();
        assert_eq!(stamp(dir.path()).unwrap(), read_stamp);
        let written = write(dir.path(), "p", &document()).unwrap();
        assert_ne!(
            read_stamp.as_deref(),
            Some(written.as_str()),
            "fresh salt and nonce"
        );
        assert_eq!(stamp(dir.path()).unwrap(), Some(written));
        assert!(matches!(
            read(dir.path(), "q"),
            Err(SyncError::WrongPassphrase)
        ));
    }

    #[test]
    fn a_tampered_or_foreign_file_fails_and_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "p", &document()).unwrap();
        let path = dir.path().join(FILE_NAME);
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x40;
        std::fs::write(&path, &bytes).unwrap();
        assert!(read(dir.path(), "p").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "reading never writes");
        std::fs::write(&path, b"hello").unwrap();
        assert!(matches!(read(dir.path(), "p"), Err(SyncError::NotSyncFile)));
    }

    #[test]
    fn a_newer_document_format_is_refused_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let mut newer = document();
        newer.format = DOCUMENT_FORMAT + 1;
        write(dir.path(), "p", &newer).unwrap();
        assert!(matches!(
            read(dir.path(), "p"),
            Err(SyncError::Newer { .. })
        ));
    }

    #[test]
    fn a_missing_folder_is_an_error_not_an_empty_sync() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("not mounted");
        assert!(matches!(read(&gone, "p"), Err(SyncError::Io { .. })));
        assert!(write(&gone, "p", &document()).is_err());
        assert!(!gone.exists());
    }
}
