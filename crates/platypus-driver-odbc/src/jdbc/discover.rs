//! Finding IBM's JDBC driver on the machine, so that a connection needs nothing typed: DBeaver
//! downloads it (and keeps the jars users add to its drivers), IBM's own clients keep it in
//! `SQLLIB`, and the app keeps the one it downloaded.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The jars found: IBM's driver, and a license jar when there is one (Db2 for z/OS needs it
/// unless the server holds the license, as most do).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoundJars {
    pub driver: Option<PathBuf>,
    pub license: Option<PathBuf>,
}

/// How deep, and how many entries, a search goes: it must end quickly on a big disk.
const MAX_DEPTH: usize = 7;
const MAX_VISITED: usize = 30_000;

fn is_jar(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jar"))
}

/// Whether `name` is IBM's driver (`Some(false)`) or a license jar (`Some(true)`).
fn kind(name: &str) -> Option<bool> {
    if !is_jar(name) {
        return None;
    }
    let name = name.to_ascii_lowercase();
    if name.starts_with("db2jcc_license") || (name.contains("license") && name.contains("jcc")) {
        Some(true)
    } else if name == "db2jcc4.jar"
        || name == "db2jcc.jar"
        || name.starts_with("jcc-")
        || name == "jcc.jar"
    {
        Some(false)
    } else {
        None
    }
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// The folders worth looking in, and the files DBeaver's driver list names.
fn places() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut folders = Vec::new();
    let mut files = Vec::new();
    for base in [
        env_dir("APPDATA"),
        env_dir("HOME").map(|h| h.join(".local").join("share")),
    ]
    .into_iter()
    .flatten()
    {
        let dbeaver = base.join("DBeaverData");
        folders.push(dbeaver.join("drivers"));
        let list = dbeaver
            .join("workspace6")
            .join(".metadata")
            .join(".config")
            .join("drivers.xml");
        if let Ok(xml) = std::fs::read_to_string(list) {
            files.extend(listed_files(&xml));
        }
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Some(base) = env_dir(var) {
            folders.push(base.join("IBM").join("SQLLIB").join("java"));
        }
    }
    folders.push(PathBuf::from(r"C:\IBM\SQLLIB\java"));
    (folders, files)
}

/// The jars DBeaver's `drivers.xml` lists (`<file … path="…"/>`), with `${home}` filled in.
fn listed_files(xml: &str) -> Vec<PathBuf> {
    let home = env_dir("USERPROFILE")
        .or_else(|| env_dir("HOME"))
        .map_or_else(String::new, |h| h.to_string_lossy().into_owned());
    let mut found = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("path=\"") {
        rest = &rest[at + "path=\"".len()..];
        let Some(end) = rest.find('"') else { break };
        let path = rest[..end].replace("${home}", &home);
        if is_jar(&path) {
            found.push(PathBuf::from(path));
        }
        rest = &rest[end..];
    }
    found
}

fn walk(dir: &Path, depth: usize, visited: &mut usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || *visited > MAX_VISITED {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        *visited += 1;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, depth + 1, visited, out);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| kind(n).is_some())
        {
            out.push(path);
        }
    }
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// The newest of `candidates` that is a file of the wanted kind.
fn newest(candidates: &[PathBuf], license: bool) -> Option<PathBuf> {
    candidates
        .iter()
        .filter(|p| p.is_file())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).and_then(kind) == Some(license))
        .max_by_key(|p| modified(p))
        .cloned()
}

/// Looks for the jars in `folders` and among the `listed` files.
fn find_in(folders: &[PathBuf], listed: &[PathBuf]) -> FoundJars {
    let mut candidates: Vec<PathBuf> = listed.to_vec();
    let mut visited = 0;
    for folder in folders {
        walk(folder, 0, &mut visited, &mut candidates);
    }
    FoundJars {
        driver: newest(&candidates, false),
        license: newest(&candidates, true),
    }
}

/// Looks for IBM's JDBC driver in the usual places and in `extra` (the folder of the one the app
/// downloaded).
#[must_use]
pub fn find(extra: &[PathBuf]) -> FoundJars {
    let (mut folders, listed) = places();
    folders.extend(extra.iter().cloned());
    find_in(&folders, &listed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_tell_the_driver_from_its_license() {
        assert_eq!(kind("db2jcc4.jar"), Some(false));
        assert_eq!(kind("jcc-12.1.4.0.jar"), Some(false));
        assert_eq!(kind("db2jcc_license_cisuz.jar"), Some(true));
        assert_eq!(kind("DB2JCC_LICENSE_CU.JAR"), Some(true));
        assert_eq!(kind("h2.jar"), None);
        assert_eq!(kind("db2jcc4.zip"), None);
    }

    #[test]
    fn the_jars_are_found_in_a_folder_tree_and_in_a_list() {
        let root = std::env::temp_dir().join(format!("platypus-find-{}", std::process::id()));
        let deep = root.join("drivers").join("maven").join("com.ibm.db2");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("jcc-12.1.4.0.jar"), b"PK").unwrap();
        std::fs::write(deep.join("other.jar"), b"PK").unwrap();
        let found = find_in(&[root.join("drivers")], &[]);
        assert_eq!(found.driver, Some(deep.join("jcc-12.1.4.0.jar")));
        assert_eq!(found.license, None);
        let license = root.join("db2jcc_license_cisuz.jar");
        std::fs::write(&license, b"PK").unwrap();
        let found = find_in(&[], &[license.clone(), root.join("gone.jar")]);
        assert_eq!(found.license, Some(license));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dbeaver_driver_lists_give_their_jar_paths() {
        let xml = r#"<file id="a" path="C:\x\db2jcc4.jar" crc="1"/><file id="b" path="x.txt"/><file path="${home}\l\db2jcc_license_cisuz.jar"/>"#;
        let files = listed_files(xml);
        assert_eq!(files.len(), 2);
        assert!(
            files[1]
                .to_string_lossy()
                .ends_with("db2jcc_license_cisuz.jar")
        );
    }
}
