//! Saving what the import dialog picked: connections, the projects they go into (new or
//! existing) with their folders, and optionally a rail folder for the new projects.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ConnectionStore, Project};
use crate::{ImportedProject, Result};

/// The colors given to new projects, the same as the project dialog's.
const PROJECT_COLORS: [&str; 8] = [
    "#6c63ff", "#d08f20", "#3d7bf7", "#a46be0", "#2f9e6e", "#e05a5a", "#1aa3b8", "#8a93a3",
];

/// Where an imported project's connections go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportInto {
    /// A new project with this name, or the name followed by a number if it is taken.
    NewProject {
        name: String,
    },
    Existing {
        id: Uuid,
    },
    /// No project: only the connections are saved.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportTarget {
    /// With only the connections picked.
    pub project: ImportedProject,
    pub into: ImportInto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRequest {
    pub targets: Vec<ImportTarget>,
    /// A rail folder with this name holds the new projects.
    pub rail_folder: Option<String>,
}

impl ConnectionStore {
    /// Saves an import in one write of the file: nothing is saved if any part is refused.
    pub fn apply_import(&mut self, request: ImportRequest) -> Result<()> {
        for target in &request.targets {
            if let ImportInto::Existing { id } = target.into {
                self.require_project(id)?;
            }
            for c in &target.project.connections {
                self.check_connection(c)?;
            }
        }
        let mut ids = std::collections::HashSet::new();
        if request
            .targets
            .iter()
            .flat_map(|t| &t.project.connections)
            .any(|c| !ids.insert(c.id) || self.connections.iter().any(|s| s.id == c.id))
        {
            return Err(crate::StoreError::Invalid(
                "these connections are saved already".to_owned(),
            ));
        }
        let before = (
            self.projects.clone(),
            self.project_folders.clone(),
            self.connections.clone(),
        );
        let mut made = Vec::new();
        for target in request.targets {
            let ids: Vec<Uuid> = target.project.connections.iter().map(|c| c.id).collect();
            // No password comes along: each connection asks for it on first use.
            self.connections
                .extend(target.project.connections.into_iter().map(|mut c| {
                    c.save_password = false;
                    c
                }));
            let index = match target.into {
                ImportInto::None => continue,
                ImportInto::Existing { id } => self.projects.iter().position(|p| p.id == id),
                ImportInto::NewProject { name } => {
                    let project = Project::new(&self.free_project_name(&name), self.free_color());
                    made.push(project.id);
                    self.projects.push(project);
                    Some(self.projects.len() - 1)
                }
            };
            let Some(project) = index.map(|i| &mut self.projects[i]) else {
                continue;
            };
            for path in &target.project.folders {
                project.folder_path(path);
            }
            for id in ids {
                project.set_member(id, true);
                if let Some(path) = target.project.placement.get(&id)
                    && let Some(folder) = project.folder_path(path)
                {
                    project.placement.insert(id, folder);
                }
            }
        }
        if let Some(name) = request.rail_folder.filter(|n| !n.trim().is_empty())
            && !made.is_empty()
        {
            self.add_project_folder(name.trim().to_owned(), made);
        }
        let written = self.write();
        if written.is_err() {
            (self.projects, self.project_folders, self.connections) = before;
        }
        written
    }

    /// `name`, or `name (2)`, `name (3)`… when a project has it already (ignoring case).
    fn free_project_name(&self, name: &str) -> String {
        let base = match name.trim() {
            "" => "Imported",
            n => n,
        };
        let taken = |n: &str| {
            let lower = n.to_lowercase();
            self.projects.iter().any(|p| p.name.to_lowercase() == lower)
        };
        let mut candidate = base.to_owned();
        let mut n = 2;
        while taken(&candidate) {
            candidate = format!("{base} ({n})");
            n += 1;
        }
        candidate
    }

    /// The first palette color no project has, or the next one round.
    fn free_color(&self) -> &'static str {
        PROJECT_COLORS
            .iter()
            .find(|c| !self.projects.iter().any(|p| p.color == **c))
            .unwrap_or(&PROJECT_COLORS[self.projects.len() % PROJECT_COLORS.len()])
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::Arc;

    use platypus_core::DriverKind;
    use uuid::Uuid;

    use super::*;
    use crate::connections::{ConnectionStore, Project, SavedConnection};
    use crate::{ImportedProject, MemorySecrets, StoreError};

    fn store(dir: &Path) -> ConnectionStore {
        ConnectionStore::open(
            dir.join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap()
    }

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| (*p).to_owned()).collect()
    }

    /// A project with `a` in `Prod/Replica` and `b` at the root.
    fn imported(name: &str) -> ImportedProject {
        let a = SavedConnection::new("a", DriverKind::Sqlite);
        let b = SavedConnection::new("b", DriverKind::Sqlite);
        ImportedProject {
            name: Some(name.to_owned()),
            folders: vec![path(&["Prod"]), path(&["Prod", "Replica"])],
            placement: BTreeMap::from([(a.id, path(&["Prod", "Replica"]))]),
            connections: vec![a, b],
            ..ImportedProject::default()
        }
    }

    fn new_project(project: ImportedProject) -> ImportTarget {
        let name = project.name.clone().unwrap_or_default();
        ImportTarget {
            project,
            into: ImportInto::NewProject { name },
        }
    }

    fn named<'a>(s: &'a ConnectionStore, name: &str) -> &'a Project {
        s.projects().iter().find(|p| p.name == name).unwrap()
    }

    /// The names from the root of the folder holding `connection` in `project`.
    fn folder_of(project: &Project, connection: Uuid) -> Vec<String> {
        let mut out = Vec::new();
        let mut at = project.placement.get(&connection).copied();
        while let Some(id) = at {
            let f = project.folders.iter().find(|f| f.id == id).unwrap();
            out.insert(0, f.name.clone());
            at = f.parent;
        }
        out
    }

    #[test]
    fn import_new_projects_into_rail_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let first = imported("Rossi");
        let a = first.connections[0].id;
        s.apply_import(ImportRequest {
            targets: vec![new_project(first), new_project(imported("Interno"))],
            rail_folder: Some("lavoro".to_owned()),
        })
        .unwrap();
        assert_eq!(s.list().len(), 4);
        let rossi = named(&s, "Rossi");
        assert_eq!(rossi.connections.len(), 2);
        assert_eq!(rossi.folders.len(), 2);
        assert_eq!(folder_of(rossi, a), path(&["Prod", "Replica"]));
        assert_ne!(rossi.color, named(&s, "Interno").color);
        let rail = &s.project_folders()[0];
        assert_eq!(rail.name, "lavoro");
        assert_eq!(rail.projects, [rossi.id, named(&s, "Interno").id]);
        // Saved: the same after reopening; no password is kept.
        let again = store(dir.path());
        assert_eq!(again.projects(), s.projects());
        assert!(again.list().iter().all(|c| !c.save_password));
    }

    #[test]
    fn import_name_taken_gets_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        s.save_project(Project::new("Interno", "#d08f20")).unwrap();
        s.apply_import(ImportRequest {
            targets: vec![
                new_project(imported("interno")),
                new_project(imported("Interno")),
            ],
            rail_folder: None,
        })
        .unwrap();
        let names: Vec<_> = s.projects().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Interno", "interno (2)", "Interno (3)"]);
        assert_eq!(s.project_folders(), []);
    }

    #[test]
    fn import_merges_folders_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let p = Project::new("Rossi", "#d08f20");
        let id = p.id;
        s.save_project(p).unwrap();
        let prod = s.create_folder(id, "prod", None).unwrap();
        let project = imported("ignored");
        let a = project.connections[0].id;
        s.apply_import(ImportRequest {
            targets: vec![ImportTarget {
                project,
                into: ImportInto::Existing { id },
            }],
            rail_folder: Some("unused".to_owned()),
        })
        .unwrap();
        let rossi = named(&s, "Rossi");
        assert_eq!(rossi.folders.len(), 2);
        assert_eq!(folder_of(rossi, a), path(&["prod", "Replica"]));
        assert_eq!(rossi.folders.iter().filter(|f| f.id == prod).count(), 1);
        // No new project, so no rail folder.
        assert_eq!(s.project_folders(), []);
    }

    #[test]
    fn import_none_saves_only_connections() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        s.apply_import(ImportRequest {
            targets: vec![ImportTarget {
                project: imported("x"),
                into: ImportInto::None,
            }],
            rail_folder: None,
        })
        .unwrap();
        assert_eq!(s.list().len(), 2);
        assert_eq!(s.projects(), []);
    }

    #[test]
    fn import_twice_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let request = ImportRequest {
            targets: vec![new_project(imported("Rossi"))],
            rail_folder: None,
        };
        s.apply_import(request.clone()).unwrap();
        // A second submit of the same pick: the connections are saved already.
        assert!(matches!(
            s.apply_import(request),
            Err(StoreError::Invalid(_))
        ));
        assert_eq!(s.list().len(), 2);
        assert_eq!(s.projects().len(), 1);
    }

    #[test]
    fn import_is_all_or_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let result = s.apply_import(ImportRequest {
            targets: vec![
                new_project(imported("Rossi")),
                ImportTarget {
                    project: imported("x"),
                    into: ImportInto::Existing { id: Uuid::new_v4() },
                },
            ],
            rail_folder: None,
        });
        assert!(matches!(result, Err(StoreError::NotFound(_))));
        assert_eq!(s.list(), []);
        assert_eq!(s.projects(), []);
        assert!(!dir.path().join("connections.toml").exists());
    }
}
