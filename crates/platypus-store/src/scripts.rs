//! Saved scripts, the read side: plain `.sql` files in a folder per project (`scripts/<project
//! id>`, or `scripts/all` for All connections), so other tools such as Git can work on them.
//! Subfolders are folders of the Scripts panel. The connection a script is linked to is kept
//! in `.links.json` in the project's folder, by relative path. The app writes them; the MCP
//! server only reads.
//!
//! Paths are relative to the project's folder, with `/`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;

use crate::{Result, StoreError};

/// The file in a project's folder that links scripts to connections.
pub const LINKS_FILE: &str = ".links.json";

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ScriptEntry {
    /// Relative path, e.g. `reports/monthly.sql`.
    pub path: String,
    pub folder: bool,
    /// The connection the script opens on.
    pub connection: Option<Uuid>,
}

/// A script's text, cut at the limit asked for.
#[derive(Debug, PartialEq, Eq)]
pub struct ScriptText {
    pub text: String,
    /// Whether the script goes on past `text`.
    pub truncated: bool,
}

/// `rel` under `root`; only plain names, so nothing reaches outside the folder, and never
/// the links file.
#[must_use]
pub fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let path = Path::new(rel);
    let plain = path.components().all(|c| matches!(c, Component::Normal(_)));
    (plain && !rel.is_empty() && rel != LINKS_FILE).then(|| root.join(path))
}

/// `path` relative to `root`, with `/`.
#[must_use]
pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The links of the project's folder; a missing or unreadable file reads as no links.
#[must_use]
pub fn read_links(root: &Path) -> BTreeMap<String, Uuid> {
    fs::read_to_string(root.join(LINKS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The scripts and folders under `root`, depth first, folders before files, by name.
/// Hidden names and files that are not `.sql` are skipped.
#[must_use]
pub fn list_scripts(root: &Path) -> Vec<ScriptEntry> {
    let mut entries = Vec::new();
    walk(root, root, &read_links(root), &mut entries);
    entries
}

fn walk(root: &Path, dir: &Path, links: &BTreeMap<String, Uuid>, out: &mut Vec<ScriptEntry>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<(bool, PathBuf)> = read
        .flatten()
        // Symbolic links are skipped: a loop would recurse, and a link may lead out of the
        // folder. `read_script` still follows one that stays inside it.
        .filter(|e| e.file_type().is_ok_and(|t| !t.is_symlink()))
        .map(|e| (e.path().is_dir(), e.path()))
        .filter(|(folder, p)| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            !name.starts_with('.')
                && (*folder || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sql")))
        })
        .collect();
    items.sort_by_key(|(folder, p)| {
        (
            !folder,
            p.file_name().map(|n| n.to_string_lossy().to_lowercase()),
        )
    });
    for (folder, path) in items {
        let rel = relative(root, &path);
        out.push(ScriptEntry {
            connection: links.get(&rel).copied(),
            path: rel,
            folder,
        });
        if folder {
            walk(root, &path, links, out);
        }
    }
}

/// The text of the script `rel` under `root`, up to `max_bytes` (cut at a character).
/// Only what [`list_scripts`] lists is a script: a `.sql` file, in no hidden folder. A path
/// that is not one, or that a link leads out of the folder, is an `InvalidInput` error.
pub fn read_script(root: &Path, rel: &str, max_bytes: usize) -> Result<ScriptText> {
    let io = |path: &Path, source| StoreError::Io {
        path: path.to_owned(),
        source,
    };
    let invalid = |path: &Path| {
        io(
            path,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a script path"),
        )
    };
    let path = resolve(root, rel).ok_or_else(|| invalid(Path::new(rel)))?;
    let hidden = rel.split('/').any(|name| name.starts_with('.'));
    let sql = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("sql"));
    if hidden || !sql {
        return Err(invalid(&path));
    }
    // A symbolic link in the folder must not lead out of it.
    let inside = fs::canonicalize(&path)
        .and_then(|p| fs::canonicalize(root).map(|r| p.starts_with(r)))
        .map_err(|e| io(&path, e))?;
    if !inside {
        return Err(invalid(&path));
    }
    let file = fs::File::open(&path).map_err(|e| io(&path, e))?;
    let mut bytes = Vec::new();
    // One byte more than asked for tells whether the script goes on.
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(&path, e))?;
    let truncated = bytes.len() > max_bytes;
    bytes.truncate(max_bytes);
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // The cut may fall inside a character; a file that is not UTF-8 reads lossy.
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            let bytes = e.into_bytes();
            if truncated && bytes.len() - valid < 4 {
                String::from_utf8_lossy(&bytes[..valid]).into_owned()
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            }
        }
    };
    Ok(ScriptText { text, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn walks_folders_first_skipping_hidden_and_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let c = Uuid::new_v4();
        write(root, "b.sql", "select 1");
        write(root, "A.SQL", "select 2");
        write(root, "notes.txt", "");
        write(root, ".hidden.sql", "");
        write(root, ".git/x.sql", "");
        write(root, "reports/monthly.sql", "select 3");
        fs::write(
            root.join(LINKS_FILE),
            format!(r#"{{"reports/monthly.sql":"{c}"}}"#),
        )
        .unwrap();
        let got: Vec<_> = list_scripts(root)
            .into_iter()
            .map(|e| (e.path, e.folder, e.connection))
            .collect();
        assert_eq!(
            got,
            [
                ("reports".to_owned(), true, None),
                ("reports/monthly.sql".to_owned(), false, Some(c)),
                ("A.SQL".to_owned(), false, None),
                ("b.sql".to_owned(), false, None),
            ]
        );
        assert_eq!(list_scripts(&root.join("missing")), Vec::new());
    }

    #[test]
    fn broken_links_read_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_links(dir.path()).is_empty());
        fs::write(dir.path().join(LINKS_FILE), "{not json").unwrap();
        assert!(read_links(dir.path()).is_empty());
    }

    #[test]
    fn paths_stay_inside_the_folder() {
        let root = Path::new("/p");
        for bad in [
            "../x.sql",
            "/etc/passwd",
            "a/../../b",
            "./a.sql",
            LINKS_FILE,
            "",
        ] {
            assert_eq!(resolve(root, bad), None, "{bad}");
        }
        assert_eq!(resolve(root, "a/b.sql"), Some(root.join("a/b.sql")));
        let dir = tempfile::tempdir().unwrap();
        for bad in [
            "../x.sql",
            LINKS_FILE,
            ".links.json",
            "notes.txt",
            ".h/x.sql",
        ] {
            assert!(read_script(dir.path(), bad, 10).is_err(), "{bad}");
        }
    }

    #[test]
    fn reads_a_script_up_to_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a/q.sql", "select 'é'");
        let all = read_script(dir.path(), "a/q.sql", 100).unwrap();
        assert_eq!(
            all,
            ScriptText {
                text: "select 'é'".to_owned(),
                truncated: false
            }
        );
        // 'é' takes two bytes, from 8 to 10: a cut at 9 drops it whole.
        let cut = read_script(dir.path(), "a/q.sql", 9).unwrap();
        assert_eq!(
            cut,
            ScriptText {
                text: "select '".to_owned(),
                truncated: true
            }
        );
        let exact = read_script(dir.path(), "a/q.sql", 11).unwrap();
        assert!(!exact.truncated);
        let missing = read_script(dir.path(), "none.sql", 10).unwrap_err();
        assert!(
            matches!(missing, StoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "far.sql", "select 1");
        let root = dir.path().join("p");
        write(&root, "real/a.sql", "select 2");
        std::os::unix::fs::symlink(".", root.join("loop")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("out")).unwrap();
        std::os::unix::fs::symlink(root.join("real/a.sql"), root.join("b.sql")).unwrap();
        let got: Vec<_> = list_scripts(&root).into_iter().map(|e| e.path).collect();
        assert_eq!(got, ["real", "real/a.sql"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "secret.sql", "select 1");
        let root = dir.path().join("p");
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.sql"), root.join("l.sql")).unwrap();
        assert!(read_script(&root, "l.sql", 100).is_err());
    }
}
