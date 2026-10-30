//! The sync file in the folder the user picked: `platypus-sync.bin`, the document sealed with
//! the sync passphrase. Writes go to a temporary file renamed over the old one, so a crash or
//! a full disk never leaves a half-written file for the other machines to read.

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

/// The document in `folder`, or `None` when no sync file is there yet. A folder that does not
/// exist is an error (a drive that is not mounted must not look like an empty one).
pub fn read(folder: &Path, passphrase: &str) -> Result<Option<Document>> {
    if !folder.is_dir() {
        return Err(SyncError::Io {
            path: folder.display().to_string(),
            message: "the sync folder was not found".to_owned(),
        });
    }
    let path = folder.join(FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(&path, &e)),
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
    Ok(Some(document))
}

/// Seals `document` and writes it to `folder`.
pub fn write(folder: &Path, passphrase: &str, document: &Document) -> Result<()> {
    if !folder.is_dir() {
        return Err(SyncError::Io {
            path: folder.display().to_string(),
            message: "the sync folder was not found".to_owned(),
        });
    }
    let plain = serde_json::to_vec(document).map_err(|e| SyncError::Invalid(e.to_string()))?;
    write_atomic(&folder.join(FILE_NAME), &crypto::seal(passphrase, &plain)?)
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
        write(dir.path(), "p", &document()).unwrap();
        let bytes = std::fs::read(dir.path().join(FILE_NAME)).unwrap();
        assert!(!bytes.windows(12).any(|w| w == b"CANARY_VALUE"));
        assert_eq!(read(dir.path(), "p").unwrap(), Some(document()));
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
