//! Saved dashboards: one JSON file per dashboard in `dashboards/<project id>` (or
//! `dashboards/all` for All connections) under the app's data folder, so that a dashboard
//! can be reopened in a later session and kept under version control. The widgets inside a
//! file are the frontend's business; the commands here only store, list, rename and delete
//! the files, and refuse names that would leave the folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::error::{CommandError, CommandResult};

const EXTENSION: &str = "json";
/// Longest name kept, so that the file name stays within what every file system accepts.
const MAX_NAME: usize = 100;

/// The folder the dashboards live in, `Err` when the data folder cannot be found.
pub struct Dashboards(pub Result<PathBuf, String>);

/// A saved dashboard, as the "Dashboards" list shows it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DashboardEntry {
    /// The file name without `.json`.
    name: String,
    /// When the file was last written, in milliseconds since the Unix epoch.
    modified: u64,
}

fn io(path: &Path, e: &std::io::Error) -> CommandError {
    CommandError::new("io", format!("{}: {e}", path.display()))
}

/// The project's folder, created on first use.
fn root(state: &Dashboards, project: Option<Uuid>) -> CommandResult<PathBuf> {
    let base = state
        .0
        .as_ref()
        .map_err(|e| CommandError::new("io", e.clone()))?;
    let dir = base.join(project.map_or_else(|| "all".to_owned(), |p| p.to_string()));
    fs::create_dir_all(&dir).map_err(|e| io(&dir, &e))?;
    Ok(dir)
}

/// Whether Windows treats `name` as a device (`CON`, `NUL`, `COM1`, ...), with or without
/// an extension: such a file cannot be created there.
fn is_reserved_device(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| {
            stem.strip_prefix(p)
                .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']))
        })
}

/// The file of the dashboard `name`; only plain names, so that the web view cannot reach
/// outside the folder, and none that Windows would refuse.
fn file(root: &Path, name: &str) -> CommandResult<PathBuf> {
    let name = name.trim();
    let bad = name.is_empty()
        || name.chars().count() > MAX_NAME
        || name.starts_with('.')
        || name.ends_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c));
    if bad || is_reserved_device(name) {
        return Err(CommandError::new(
            "invalid",
            format!("not a dashboard name: {name}"),
        ));
    }
    Ok(root.join(format!("{name}.{EXTENSION}")))
}

/// The dashboards of `root`, by name.
fn list(root: &Path) -> Vec<DashboardEntry> {
    let Ok(read) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut entries: Vec<DashboardEntry> = read
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if !path.is_file() || path.extension()?.to_str()? != EXTENSION {
                return None;
            }
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            Some(DashboardEntry {
                name: path.file_stem()?.to_string_lossy().into_owned(),
                modified,
            })
        })
        .collect();
    entries.sort_by_key(|e| e.name.to_lowercase());
    entries
}

fn read(root: &Path, name: &str) -> CommandResult<String> {
    let path = file(root, name)?;
    fs::read_to_string(&path).map_err(|e| io(&path, &e))
}

/// Writes `contents`, which must be a JSON object, as the dashboard `name`.
fn write(root: &Path, name: &str, contents: &str) -> CommandResult<()> {
    let path = file(root, name)?;
    match serde_json::from_str::<serde_json::Value>(contents) {
        Ok(v) if v.is_object() => {}
        _ => {
            return Err(CommandError::new(
                "invalid",
                "a dashboard is a JSON object".to_owned(),
            ));
        }
    }
    // Written beside the target and renamed over it, so that a crash mid-write cannot leave
    // a truncated dashboard; the `.tmp` extension keeps it out of the list.
    let aside = path.with_extension(format!("{EXTENSION}.tmp"));
    fs::write(&aside, contents)
        .and_then(|()| fs::rename(&aside, &path))
        .map_err(|e| {
            let _ = fs::remove_file(&aside);
            io(&path, &e)
        })
}

fn rename(root: &Path, from: &str, to: &str) -> CommandResult<()> {
    let (source, target) = (file(root, from)?, file(root, to)?);
    // On a case-insensitive file system `a.json` to `A.json` finds the file itself.
    let same_file = fs::canonicalize(&source).ok() == fs::canonicalize(&target).ok();
    // A name that differs only by case is the same name on such a file system, and would
    // be confusing on any other, so it counts as taken unless it is the dashboard itself.
    let lower = to.trim().to_lowercase();
    let clash = list(root)
        .iter()
        .any(|e| e.name.to_lowercase() == lower && e.name != from.trim());
    if clash || (target.exists() && !same_file) {
        return Err(CommandError::new("exists", format!("{to} already exists")));
    }
    fs::rename(&source, &target).map_err(|e| io(&source, &e))
}

fn delete(root: &Path, name: &str) -> CommandResult<()> {
    let path = file(root, name)?;
    fs::remove_file(&path).map_err(|e| io(&path, &e))
}

/// The saved dashboards of a project, by name.
#[tauri::command]
pub fn list_dashboards(
    state: State<'_, Dashboards>,
    project: Option<Uuid>,
) -> CommandResult<Vec<DashboardEntry>> {
    Ok(list(&root(&state, project)?))
}

/// The JSON text of a saved dashboard.
#[tauri::command]
pub fn read_dashboard(
    state: State<'_, Dashboards>,
    project: Option<Uuid>,
    name: String,
) -> CommandResult<String> {
    read(&root(&state, project)?, &name)
}

/// Saves a dashboard under `name`, replacing the one there.
#[tauri::command]
pub fn save_dashboard(
    state: State<'_, Dashboards>,
    project: Option<Uuid>,
    name: String,
    contents: String,
) -> CommandResult<()> {
    write(&root(&state, project)?, &name, &contents)
}

/// Renames a saved dashboard; refused when `to` is taken.
#[tauri::command]
pub fn rename_dashboard(
    state: State<'_, Dashboards>,
    project: Option<Uuid>,
    from: String,
    to: String,
) -> CommandResult<()> {
    rename(&root(&state, project)?, &from, &to)
}

/// Deletes a saved dashboard.
#[tauri::command]
pub fn delete_dashboard(
    state: State<'_, Dashboards>,
    project: Option<Uuid>,
    name: String,
) -> CommandResult<()> {
    delete(&root(&state, project)?, &name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(root: &Path) -> Vec<String> {
        list(root).into_iter().map(|e| e.name).collect()
    }

    #[test]
    fn saves_lists_renames_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Sales", r#"{"widgets":[]}"#).unwrap();
        write(root, "archive", "{}").unwrap();
        fs::write(root.join("notes.txt"), "").unwrap();
        assert_eq!(names(root), ["archive", "Sales"]);
        assert_eq!(read(root, "Sales").unwrap(), r#"{"widgets":[]}"#);
        // Saving again replaces.
        write(root, "Sales", r#"{"widgets":[1]}"#).unwrap();
        assert_eq!(read(root, "Sales").unwrap(), r#"{"widgets":[1]}"#);

        rename(root, "Sales", "Revenue").unwrap();
        assert_eq!(names(root), ["archive", "Revenue"]);
        let taken = rename(root, "archive", "Revenue").unwrap_err();
        assert_eq!(serde_json::to_value(taken).unwrap()["kind"], "exists");
        rename(root, "Revenue", "revenue").unwrap();
        assert!(root.join("revenue.json").exists());

        delete(root, "revenue").unwrap();
        assert_eq!(names(root), ["archive"]);
        assert!(delete(root, "revenue").is_err(), "already gone");
        assert!(read(root, "revenue").is_err());
    }

    #[test]
    fn stays_inside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        for bad in [
            "../x", "a/b", "a\\b", "", "  ", ".hidden", "x.", "a:b", "a\nb",
        ] {
            assert!(file(dir.path(), bad).is_err(), "{bad:?}");
            assert!(write(dir.path(), bad, "{}").is_err(), "{bad:?}");
        }
        assert!(file(dir.path(), &"x".repeat(MAX_NAME + 1)).is_err());
        assert!(file(dir.path(), "Q3 sales (EU)").is_ok());
    }

    #[test]
    fn refuses_windows_device_names() {
        let dir = tempfile::tempdir().unwrap();
        for bad in [
            "con", "CON", "Nul", "prn", "aux", "COM1", "lpt9", "con.x", "aux.tar",
        ] {
            assert!(file(dir.path(), bad).is_err(), "{bad:?}");
        }
        for ok in ["COM0", "COM10", "console", "com", "my.con", "LPT"] {
            assert!(file(dir.path(), ok).is_ok(), "{ok:?}");
        }
    }

    #[test]
    fn writes_atomically_and_refuses_case_only_clashes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Sales", "{}").unwrap();
        write(root, "Sales", r#"{"a":1}"#).unwrap();
        let files: Vec<_> = fs::read_dir(root).unwrap().flatten().collect();
        assert_eq!(files.len(), 1, "no temporary file left behind");
        write(root, "other", "{}").unwrap();
        let clash = rename(root, "other", "SALES").unwrap_err();
        assert_eq!(serde_json::to_value(clash).unwrap()["kind"], "exists");
        rename(root, "Sales", "sales").unwrap();
    }

    #[test]
    fn keeps_only_json_objects() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["", "not json", "[]", "3"] {
            assert!(write(dir.path(), "d", bad).is_err(), "{bad:?}");
        }
        assert_eq!(names(dir.path()).len(), 0);
    }

    #[test]
    fn projects_have_their_own_folder() {
        let dir = tempfile::tempdir().unwrap();
        let state = Dashboards(Ok(dir.path().to_owned()));
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        write(&root(&state, Some(a)).unwrap(), "d", "{}").unwrap();
        assert_eq!(list(&root(&state, Some(a)).unwrap()).len(), 1);
        assert_eq!(list(&root(&state, Some(b)).unwrap()).len(), 0);
        assert_eq!(list(&root(&state, None).unwrap()).len(), 0);
        assert!(root(&Dashboards(Err("no folder".to_owned())), None).is_err());
    }
}
