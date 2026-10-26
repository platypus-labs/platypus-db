//! A DBeaver project export (File › Export › DBeaver project): a zip with `meta.xml`,
//! which lists the projects, and each project's folder under `projects/<name>/`. Only the
//! connections are read (`.dbeaver/data-sources*.json`, with the saved logins of
//! `.dbeaver/credentials-config.json`), not the scripts. A zip of workspace folders
//! (`<project>/.dbeaver/…`) is read the same way.

use std::collections::HashMap;
use std::io::Cursor;

use super::{Credentials, ImportedProject, dbeaver, unescape};

/// Far more than any `data-sources.json`: a bigger entry is refused, not unpacked.
const MAX_ENTRY: u64 = 16 << 20;

/// The projects with connections, in the order of `meta.xml` (by name without it).
pub(super) fn projects(bytes: &[u8]) -> Result<Vec<ImportedProject>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut order = Vec::new();
    // Project, entry name, text.
    let mut sources: Vec<(String, String, String)> = Vec::new();
    let mut logins: HashMap<String, Credentials> = HashMap::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        // Java writes `/`; a zip made on Windows may hold `\`.
        let name = file.name().map_err(|e| e.to_string())?.replace('\\', "/");
        if name.trim_start_matches("./") == "meta.xml" {
            order = project_names(&read(&mut file)?);
        } else if let Some((project, file_name)) = dbeaver_file(&name) {
            let project = project.to_owned();
            if file_name == "credentials-config.json" {
                logins.insert(project, Credentials::of(Some(&read_bytes(&mut file)?)));
            } else if is_data_sources(file_name) {
                sources.push((project, name.clone(), read(&mut file)?));
            }
        }
    }
    if sources.is_empty() {
        return Err(
            "no DBeaver connections in this archive: it holds no .dbeaver/data-sources*.json"
                .to_owned(),
        );
    }
    let mut found: Vec<ImportedProject> = Vec::new();
    for (project, entry, text) in sources {
        let credentials = logins.get(&project).unwrap_or(&Credentials::Missing);
        let imported = dbeaver(&text, credentials).map_err(|e| format!("{entry}: {e}"))?;
        match found
            .iter_mut()
            .find(|p| p.name.as_deref() == Some(project.as_str()))
        {
            Some(existing) => existing.merge(imported),
            None => found.push(ImportedProject {
                name: Some(project),
                ..imported
            }),
        }
    }
    // A project whose connections are all left out stays, to tell why.
    found.retain(|p| !p.connections.is_empty() || !p.skipped.is_empty());
    found.sort_by_key(|p| {
        let name = p.name.clone().unwrap_or_default();
        (
            order.iter().position(|o| *o == name).unwrap_or(usize::MAX),
            name,
        )
    });
    Ok(found)
}

/// The project and the file name of a `projects/<name>/.dbeaver/<file>` entry: the
/// project is the folder that holds `.dbeaver`, at any depth.
fn dbeaver_file(entry: &str) -> Option<(&str, &str)> {
    let (dir, file) = entry.rsplit_once('/')?;
    let (parent, dot) = dir.rsplit_once('/')?;
    let project = parent.rsplit('/').next()?;
    (dot == ".dbeaver" && !project.is_empty() && project != ".").then_some((project, file))
}

/// `data-sources.json`, or one more such file (`data-sources-2.json`).
fn is_data_sources(file: &str) -> bool {
    file.starts_with("data-sources")
        && std::path::Path::new(file)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

fn read_bytes(file: &mut impl std::io::Read) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    file.take(MAX_ENTRY + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_ENTRY {
        return Err("an entry of the archive is too large".to_owned());
    }
    Ok(bytes)
}

fn read(file: &mut impl std::io::Read) -> Result<String, String> {
    String::from_utf8(read_bytes(file)?).map_err(|e| e.to_string())
}

/// The `name` of each `<project …>` tag.
fn project_names(xml: &str) -> Vec<String> {
    xml.split("<project ")
        .skip(1)
        .filter_map(|tag| {
            let start = tag.find("name=\"")? + "name=\"".len();
            let end = tag[start..].find('"')? + start;
            Some(unescape(&tag[start..end]))
        })
        .collect()
}
