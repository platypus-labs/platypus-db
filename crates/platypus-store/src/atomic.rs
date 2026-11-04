//! Putting a finished temporary file in the place of a saved one.

use std::io;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use tempfile::NamedTempFile;

/// How many times, and how long apart at first, a refused replacement is tried again.
const ATTEMPTS: u32 = 8;
const FIRST_WAIT: Duration = Duration::from_millis(10);

/// Replaces `path` with `file`. On Windows the replacement is refused while another program
/// holds `path` open without sharing its deletion, as antivirus scanners, backup tools and some
/// editors do for a moment: it is tried again for about a second before it counts as an error.
pub(crate) fn replace(file: NamedTempFile, path: &Path) -> io::Result<()> {
    let (mut file, mut wait) = (file, FIRST_WAIT);
    for _ in 1..ATTEMPTS {
        match file.persist(path) {
            Ok(_) => return Ok(()),
            Err(e) if e.error.kind() == io::ErrorKind::PermissionDenied => {
                file = e.file;
                sleep(wait);
                wait *= 2;
            }
            Err(e) => return Err(e.error),
        }
    }
    file.persist(path).map(|_| ()).map_err(|e| e.error)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn temp_with(dir: &Path, text: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new_in(dir).unwrap();
        file.write_all(text.as_bytes()).unwrap();
        file
    }

    #[test]
    fn replaces_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved.toml");
        replace(temp_with(dir.path(), "one"), &path).unwrap();
        replace(temp_with(dir.path(), "two"), &path).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "two");
    }

    #[cfg(windows)]
    #[test]
    fn waits_for_a_program_that_holds_the_file_for_a_moment() {
        use std::os::windows::fs::OpenOptionsExt as _;

        const FILE_SHARE_READ: u32 = 1;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved.toml");
        std::fs::write(&path, "one").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        let reader = std::thread::spawn(move || {
            sleep(Duration::from_millis(100));
            drop(held);
        });
        replace(temp_with(dir.path(), "two"), &path).unwrap();
        reader.join().unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "two");
    }

    #[cfg(windows)]
    #[test]
    fn a_file_held_for_long_is_still_an_error() {
        use std::os::windows::fs::OpenOptionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved.toml");
        std::fs::write(&path, "one").unwrap();
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let error = replace(temp_with(dir.path(), "two"), &path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}
