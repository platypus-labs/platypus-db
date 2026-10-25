//! A DBeaver project export (File › Export › DBeaver project): a zip with `meta.xml`,
//! which lists the projects, and each project's folder under `projects/<name>/`. Only the
//! connections are read (`.dbeaver/data-sources.json`), not the scripts.

use std::io::Cursor;

use super::{ImportedProject, dbeaver, unescape};

const SOURCES: &str = "/.dbeaver/data-sources.json";

/// The projects with connections, in the order of `meta.xml` (by name without it).
pub(super) fn projects(bytes: &[u8]) -> Result<Vec<ImportedProject>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut order = Vec::new();
    let mut found = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().map_err(|e| e.to_string())?.into_owned();
        if name == "meta.xml" {
            order = project_names(&read(&mut file)?);
        } else if let Some(project) = name
            .strip_prefix("projects/")
            .and_then(|rest| rest.strip_suffix(SOURCES))
            .filter(|p| !p.contains('/'))
        {
            let mut imported = dbeaver(&read(&mut file)?)?;
            imported.name = Some(project.to_owned());
            found.push(imported);
        }
    }
    found.retain(|p| !p.connections.is_empty());
    found.sort_by_key(|p| {
        let name = p.name.clone().unwrap_or_default();
        (
            order.iter().position(|o| *o == name).unwrap_or(usize::MAX),
            name,
        )
    });
    Ok(found)
}

fn read(file: &mut impl std::io::Read) -> Result<String, String> {
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|e| e.to_string())?;
    Ok(text)
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
