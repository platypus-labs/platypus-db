//! Saved scripts: plain `.sql` files in a folder per project (`scripts/<project id>`, or
//! `scripts/all` for All connections) under the app's data folder, so other tools such as
//! Git can work on them. Subfolders are folders of the Scripts panel. The connection a
//! script is linked to is kept in `.links.json` in the project's folder, by relative path.
//!
//! Paths the commands take and return are relative to the project's folder, with `/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use platypus_store::{
    LINKS_FILE, ScriptEntry, list_scripts as walk_scripts, read_script_links as read_links,
    resolve_script, script_relative as relative,
};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::AppState;
use crate::error::{CommandError, CommandResult};

#[derive(Debug, Serialize)]
pub struct ScriptList {
    /// The project's folder, absolute: the scripts open from here.
    root: PathBuf,
    /// Depth first, folders before files, by name.
    entries: Vec<ScriptEntry>,
}

fn io(path: &Path, e: &std::io::Error) -> CommandError {
    CommandError::new("io", format!("{}: {e}", path.display()))
}

/// The project's folder, created on first use.
pub(super) fn root(state: &AppState, project: Option<Uuid>) -> CommandResult<PathBuf> {
    // A shared project keeps its scripts in the shared folder, so version control or the
    // shared drive sees them.
    if let Some(project) = project
        && let Some(link) = state.with_store(|s| {
            Ok(s.projects()
                .iter()
                .find(|p| p.id == project)
                .and_then(|p| p.shared.clone()))
        })?
    {
        return super::shared_projects::scripts_root(&link.path);
    }
    let base = state
        .scripts
        .as_ref()
        .map_err(|e| CommandError::new("io", e.clone()))?;
    let dir = base.join(project.map_or_else(|| "all".to_owned(), |p| p.to_string()));
    fs::create_dir_all(&dir).map_err(|e| io(&dir, &e))?;
    Ok(dir)
}

/// `rel` under `root`; only plain names, so the web view cannot reach outside the folder.
fn resolve(root: &Path, rel: &str) -> CommandResult<PathBuf> {
    resolve_script(root, rel)
        .ok_or_else(|| CommandError::new("invalid", format!("not a script path: {rel}")))
}

fn write_links(root: &Path, links: &BTreeMap<String, Uuid>) -> CommandResult<()> {
    let path = root.join(LINKS_FILE);
    let text =
        serde_json::to_string_pretty(links).map_err(|e| CommandError::new("io", e.to_string()))?;
    fs::write(&path, text).map_err(|e| io(&path, &e))
}

fn list(root: &Path) -> ScriptList {
    ScriptList {
        root: root.to_owned(),
        entries: walk_scripts(root),
    }
}

#[tauri::command]
pub fn list_scripts(
    state: State<'_, AppState>,
    project: Option<Uuid>,
) -> CommandResult<ScriptList> {
    Ok(list(&root(&state, project)?))
}

/// A new empty script in `folder` (`""` for the top), named `name.sql` or, when taken,
/// `name 2.sql`…; returns its relative path.
fn create(
    root: &Path,
    folder: &str,
    name: &str,
    connection: Option<Uuid>,
) -> CommandResult<String> {
    let dir = if folder.is_empty() {
        root.to_owned()
    } else {
        resolve(root, folder)?
    };
    let stem = name.trim().trim_end_matches(".sql");
    let stem = if stem.is_empty() { "Script" } else { stem };
    let mut n = 1;
    let path = loop {
        let file = if n == 1 {
            format!("{stem}.sql")
        } else {
            format!("{stem} {n}.sql")
        };
        let path = dir.join(&file);
        if !path.exists() {
            break path;
        }
        n += 1;
    };
    let rel = relative(root, &path);
    resolve(root, &rel)?;
    fs::write(&path, "").map_err(|e| io(&path, &e))?;
    if let Some(connection) = connection {
        let mut links = read_links(root);
        links.insert(rel.clone(), connection);
        write_links(root, &links)?;
    }
    Ok(rel)
}

/// Writes the scripts `files` (name, text) into `folder`, linked to `connection`, except
/// those already there, which keep the user's changes; returns their relative paths. One
/// already there is linked again when its link is gone or names no connection that `saved`
/// knows (one deleted since), so it does not open on another.
pub(super) fn add_linked(
    root: &Path,
    folder: &str,
    files: &[(&str, &str)],
    connection: Uuid,
    saved: impl Fn(Uuid) -> bool,
) -> CommandResult<Vec<String>> {
    let dir = resolve(root, folder)?;
    fs::create_dir_all(&dir).map_err(|e| io(&dir, &e))?;
    let mut links = read_links(root);
    let mut paths = Vec::with_capacity(files.len());
    for (name, text) in files {
        let path = dir.join(name);
        let rel = relative(root, &path);
        if !path.exists() {
            fs::write(&path, text).map_err(|e| io(&path, &e))?;
            links.insert(rel.clone(), connection);
        } else if !links.get(&rel).is_some_and(|c| saved(*c)) {
            links.insert(rel.clone(), connection);
        }
        paths.push(rel);
    }
    write_links(root, &links)?;
    Ok(paths)
}

#[tauri::command]
pub fn create_script(
    state: State<'_, AppState>,
    project: Option<Uuid>,
    folder: String,
    name: String,
    connection: Option<Uuid>,
) -> CommandResult<String> {
    create(&root(&state, project)?, &folder, &name, connection)
}

#[tauri::command]
pub fn create_script_folder(
    state: State<'_, AppState>,
    project: Option<Uuid>,
    path: String,
) -> CommandResult<()> {
    let dir = resolve(&root(&state, project)?, &path)?;
    if dir.exists() {
        return Err(CommandError::new(
            "exists",
            format!("{path} already exists"),
        ));
    }
    fs::create_dir_all(&dir).map_err(|e| io(&dir, &e))
}

/// Renames or moves a script or folder; links follow it.
fn rename(root: &Path, from: &str, to: &str) -> CommandResult<()> {
    let (source, target) = (resolve(root, from)?, resolve(root, to)?);
    // On a case-insensitive file system `a.sql` to `A.sql` finds the file itself.
    let same_file = fs::canonicalize(&source).ok() == fs::canonicalize(&target).ok();
    if target.exists() && !same_file {
        return Err(CommandError::new("exists", format!("{to} already exists")));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| io(parent, &e))?;
    }
    fs::rename(&source, &target).map_err(|e| io(&source, &e))?;
    let links = read_links(root);
    let moved: BTreeMap<_, _> = links
        .into_iter()
        .map(|(path, c)| match path.strip_prefix(from) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => (format!("{to}{rest}"), c),
            _ => (path, c),
        })
        .collect();
    write_links(root, &moved)
}

#[tauri::command]
pub fn rename_script(
    state: State<'_, AppState>,
    project: Option<Uuid>,
    from: String,
    to: String,
) -> CommandResult<()> {
    rename(&root(&state, project)?, &from, &to)
}

/// Deletes a script, or a folder with everything in it.
fn delete(root: &Path, path: &str) -> CommandResult<()> {
    let target = resolve(root, path)?;
    if target.is_dir() {
        fs::remove_dir_all(&target)
    } else {
        fs::remove_file(&target)
    }
    .map_err(|e| io(&target, &e))?;
    let mut links = read_links(root);
    links.retain(|p, _| p != path && !p.starts_with(&format!("{path}/")));
    write_links(root, &links)
}

#[tauri::command]
pub fn delete_script(
    state: State<'_, AppState>,
    project: Option<Uuid>,
    path: String,
) -> CommandResult<()> {
    delete(&root(&state, project)?, &path)
}

/// Links a script to a connection, or (`None`) unlinks it.
#[tauri::command]
pub fn link_script(
    state: State<'_, AppState>,
    project: Option<Uuid>,
    path: String,
    connection: Option<Uuid>,
) -> CommandResult<()> {
    let root = root(&state, project)?;
    resolve(&root, &path)?;
    let mut links = read_links(&root);
    match connection {
        Some(c) => links.insert(path, c),
        None => links.remove(&path),
    };
    write_links(&root, &links)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &Path) -> Vec<(String, bool, Option<Uuid>)> {
        list(root)
            .entries
            .into_iter()
            .map(|e| (e.path, e.folder, e.connection))
            .collect()
    }

    #[test]
    fn creates_renames_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let c = Uuid::new_v4();
        fs::create_dir(root.join("reports")).unwrap();
        assert_eq!(
            create(root, "reports", "Monthly", Some(c)).unwrap(),
            "reports/Monthly.sql"
        );
        assert_eq!(
            create(root, "reports", "Monthly.sql", None).unwrap(),
            "reports/Monthly 2.sql"
        );
        assert_eq!(create(root, "", "", None).unwrap(), "Script.sql");
        fs::write(root.join("notes.txt"), "").unwrap();
        assert_eq!(
            paths(root),
            [
                ("reports".to_owned(), true, None),
                ("reports/Monthly 2.sql".to_owned(), false, None),
                ("reports/Monthly.sql".to_owned(), false, Some(c)),
                ("Script.sql".to_owned(), false, None),
            ]
        );
        // Links follow a renamed folder.
        rename(root, "reports", "old/reports").unwrap();
        assert_eq!(read_links(root).get("old/reports/Monthly.sql"), Some(&c));
        assert!(rename(root, "Script.sql", "old").is_err(), "taken");
        rename(root, "Script.sql", "script.sql").unwrap();
        assert!(root.join("script.sql").exists());
        delete(root, "old").unwrap();
        assert!(read_links(root).is_empty());
        assert_eq!(paths(root).len(), 1);
    }

    #[test]
    fn stays_inside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["../x.sql", "/etc/passwd", "a/../../b", LINKS_FILE, ""] {
            assert!(resolve(dir.path(), bad).is_err(), "{bad}");
        }
        assert!(create(dir.path(), "..", "x", None).is_err());
    }
}
