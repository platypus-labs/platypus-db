//! A DBeaver project export (File › Export › DBeaver project): a zip with `meta.xml`,
//! which lists the projects, and each project's folder under `projects/<name>/`. Only the
//! connections are read (`.dbeaver/data-sources*.json`), not the scripts. A zip of
//! workspace folders (`<project>/.dbeaver/…`) is read the same way.

use std::io::Cursor;

use super::{ImportedProject, dbeaver, unescape};

/// Far more than any `data-sources.json`: a bigger entry is refused, not unpacked.
const MAX_ENTRY: u64 = 16 << 20;

/// The projects with connections, in the order of `meta.xml` (by name without it).
pub(super) fn projects(bytes: &[u8]) -> Result<Vec<ImportedProject>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut order = Vec::new();
    let mut found = Vec::new();
    let mut sources = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        // Java writes `/`; a zip made on Windows may hold `\`.
        let name = file.name().map_err(|e| e.to_string())?.replace('\\', "/");
        if name.trim_start_matches("./") == "meta.xml" {
            order = project_names(&read(&mut file)?);
        } else if let Some(project) = sources_of(&name) {
            sources += 1;
            let imported = dbeaver(&read(&mut file)?).map_err(|e| format!("{name}: {e}"))?;
            match found
                .iter_mut()
                .find(|p: &&mut ImportedProject| p.name.as_deref() == Some(project))
            {
                Some(existing) => existing.merge(imported),
                None => found.push(ImportedProject {
                    name: Some(project.to_owned()),
                    ..imported
                }),
            }
        }
    }
    if sources == 0 {
        return Err(
            "no DBeaver connections in this archive: it holds no .dbeaver/data-sources*.json"
                .to_owned(),
        );
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

/// The project of a `projects/<name>/.dbeaver/data-sources*.json` entry: the folder that
/// holds `.dbeaver`, at any depth.
fn sources_of(entry: &str) -> Option<&str> {
    let (dir, file) = entry.rsplit_once('/')?;
    let (parent, dot) = dir.rsplit_once('/')?;
    let project = parent.rsplit('/').next()?;
    let named = file.starts_with("data-sources")
        && std::path::Path::new(file)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    (named && dot == ".dbeaver" && !project.is_empty() && project != ".").then_some(project)
}

fn read(file: &mut impl std::io::Read) -> Result<String, String> {
    use std::io::Read as _;
    let mut text = String::new();
    file.take(MAX_ENTRY + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() as u64 > MAX_ENTRY {
        return Err("an entry of the archive is too large".to_owned());
    }
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
