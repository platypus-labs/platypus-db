//! Saving what the import dialog picked: connections, the projects they go into (new or
//! existing) with their folders, and optionally a rail folder for the new projects.

use std::collections::{BTreeMap, HashSet};

use platypus_core::Secret;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ConnectionStore, Project, SavedSsh};
use crate::{ImportedProject, Result, StoreError};

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
    /// Saves the passwords that came with the connections (each project's `secrets`), as
    /// if typed in the connection dialog; otherwise each connection asks on first use.
    #[serde(default)]
    pub passwords: bool,
    /// Imported connections that replace a saved one, imported id → saved id. The saved
    /// connection keeps its id (so its projects, tabs and history) and the passwords the
    /// file does not bring.
    #[serde(default)]
    pub replace: BTreeMap<Uuid, Uuid>,
}

impl ConnectionStore {
    /// Saves an import in one write of the file: nothing is saved if any part is refused.
    pub fn apply_import(&mut self, mut request: ImportRequest) -> Result<()> {
        let (replaced, unused_secrets) = self.adopt_replaced(&mut request)?;
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
            .any(|c| {
                !ids.insert(c.id)
                    || (!replaced.contains(&c.id) && self.connections.iter().any(|s| s.id == c.id))
            })
        {
            return Err(crate::StoreError::Invalid(
                "these connections are saved already".to_owned(),
            ));
        }
        let secrets = if request.passwords {
            take_secrets(&mut request.targets)
        } else {
            Vec::new()
        };
        let secrets_before = self.store_secrets(&secrets)?;
        let before = (
            self.projects.clone(),
            self.project_folders.clone(),
            self.connections.clone(),
        );
        let mut made = Vec::new();
        for target in request.targets {
            let ids: Vec<Uuid> = target.project.connections.iter().map(|c| c.id).collect();
            for mut c in target.project.connections {
                // A connection without a saved password asks for it on first use; a
                // replaced one keeps its own.
                let old = self.connections.iter().position(|s| s.id == c.id);
                c.save_password = secrets.iter().any(|(id, _)| *id == c.id)
                    || old.is_some_and(|i| self.connections[i].save_password);
                match old {
                    Some(i) => self.connections[i] = c,
                    None => self.connections.push(c),
                }
            }
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
            self.restore_secrets(&secrets_before);
            return written;
        }
        for id in unused_secrets {
            let _ = self.secrets.delete(id);
        }
        Ok(())
    }

    /// Gives each imported connection of `request.replace` the id of the saved one it
    /// replaces, and the saved tunnel's secret ids hop by hop, so the saved SSH passwords
    /// stay unless the file brings new ones. Returns the replaced ids, and the secret ids
    /// of saved hops the new tunnel no longer has.
    fn adopt_replaced(&self, request: &mut ImportRequest) -> Result<(HashSet<Uuid>, Vec<Uuid>)> {
        let mut replaced = HashSet::new();
        let mut unused = Vec::new();
        for project in request.targets.iter_mut().map(|t| &mut t.project) {
            for c in &mut project.connections {
                let Some(&to) = request.replace.get(&c.id) else {
                    continue;
                };
                let saved = self.get(to).ok_or(StoreError::NotFound(to))?;
                let from = std::mem::replace(&mut c.id, to);
                replaced.insert(to);
                if let Some(s) = project.secrets.remove(&from) {
                    project.secrets.insert(to, s);
                }
                if let Some(path) = project.placement.remove(&from) {
                    project.placement.insert(to, path);
                }
                let mut hop = c.ssh.as_mut();
                for old in saved.ssh.iter().flat_map(SavedSsh::chain) {
                    match hop {
                        Some(new) => {
                            new.secret_id = old.secret_id;
                            hop = new.jump.as_deref_mut();
                        }
                        None => unused.push(old.secret_id),
                    }
                }
            }
        }
        Ok((replaced, unused))
    }

    /// Saves the secrets, or none of them. Returns what each id held before, to put back.
    fn store_secrets(&self, secrets: &[(Uuid, Secret)]) -> Result<Vec<(Uuid, Option<Secret>)>> {
        let mut before = Vec::new();
        for (id, secret) in secrets {
            let stored = self
                .secrets
                .get(*id)
                .and_then(|old| self.secrets.set(*id, secret).map(|()| old));
            match stored {
                Ok(old) => before.push((*id, old)),
                Err(e) => {
                    self.restore_secrets(&before);
                    return Err(e);
                }
            }
        }
        Ok(before)
    }

    fn restore_secrets(&self, before: &[(Uuid, Option<Secret>)]) {
        for (id, old) in before.iter().rev() {
            let _ = self.set_secret(*id, old.as_ref());
        }
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

/// The secrets to save for the connections of `targets`, under the ids the store keeps
/// them by: the connection's own for its password, each SSH hop's `secret_id` for the
/// tunnel's. The connection's password comes first.
fn take_secrets(targets: &mut [ImportTarget]) -> Vec<(Uuid, Secret)> {
    let mut out = Vec::new();
    for target in targets {
        let mut secrets = std::mem::take(&mut target.project.secrets);
        for c in &target.project.connections {
            let Some(s) = secrets.remove(&c.id) else {
                continue;
            };
            if let Some(password) = s.password {
                out.push((c.id, password));
            }
            let hops = c.ssh.iter().flat_map(SavedSsh::chain);
            let hop_secrets = std::iter::once(s.ssh).chain(s.jumps);
            for (hop, secret) in hops.zip(hop_secrets) {
                if let Some(secret) = secret {
                    out.push((hop.secret_id, secret));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::Arc;

    use platypus_core::DriverKind;
    use uuid::Uuid;

    use super::*;
    use crate::connections::{ConnectionStore, Project, SavedConnection, SavedSsh, SshAuthKind};
    use crate::{ImportedProject, MemorySecrets, SecretStore as _, StoreError};

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
            passwords: false,
            replace: BTreeMap::new(),
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

    fn ssh(host: &str, jump: Option<SavedSsh>) -> SavedSsh {
        SavedSsh {
            host: host.to_owned(),
            port: 22,
            user: "ops".to_owned(),
            auth: SshAuthKind::Password,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: jump.map(Box::new),
        }
    }

    /// `imported` where `a` comes with a password and an SSH tunnel through a jump host,
    /// each with its own.
    fn with_secrets() -> ImportedProject {
        let mut project = imported("Rossi");
        let a = &mut project.connections[0];
        a.ssh = Some(ssh("bastion", Some(ssh("jump", None))));
        project.secrets.insert(
            a.id,
            crate::ImportedSecrets {
                password: Some(Secret::new("db-pass")),
                ssh: Some(Secret::new("ssh-pass")),
                jumps: vec![Some(Secret::new("jump-pass"))],
            },
        );
        project
    }

    #[test]
    fn import_replaces_a_saved_connection_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        // Saved: `old` in a project, with a password and a two-hop tunnel.
        let mut old = SavedConnection::new("old", DriverKind::Sqlite);
        old.save_password = true;
        old.ssh = Some(ssh("bastion", Some(ssh("jump", None))));
        let (id, hops) = (old.id, old.ssh.as_ref().unwrap().clone());
        let other = SavedConnection::new("other", DriverKind::Sqlite);
        s.save(old, crate::PasswordUpdate::Set(Secret::new("old-pass")))
            .unwrap();
        s.save(other.clone(), crate::PasswordUpdate::Keep).unwrap();
        s.set_ssh_secret(hops.secret_id, Some(&Secret::new("old-ssh")))
            .unwrap();
        let jump_secret = hops.jump.as_ref().unwrap().secret_id;
        s.set_ssh_secret(jump_secret, Some(&Secret::new("old-jump")))
            .unwrap();
        let mut p = Project::new("Team", "#d08f20");
        p.connections = vec![id];
        s.save_project(p).unwrap();

        // The file's `a` replaces it with a one-hop tunnel and a new SSH password only.
        let mut project = imported("Rossi");
        project.connections[0].ssh = Some(ssh("bastion2", None));
        let (a, b) = (project.connections[0].id, project.connections[1].id);
        project.secrets.insert(
            a,
            crate::ImportedSecrets {
                ssh: Some(Secret::new("new-ssh")),
                ..crate::ImportedSecrets::default()
            },
        );
        s.apply_import(ImportRequest {
            targets: vec![new_project(project)],
            rail_folder: None,
            passwords: true,
            replace: BTreeMap::from([(a, id)]),
        })
        .unwrap();
        // In place: same id, same spot, still in its project, now in the new one too.
        let names: Vec<_> = s.list().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "other", "b"]);
        let c = s.get(id).unwrap();
        assert!(s.get(a).is_none() && s.get(b).is_some());
        assert_eq!(c.ssh.as_ref().unwrap().host, "bastion2");
        assert!(named(&s, "Team").connections.contains(&id));
        assert_eq!(
            folder_of(named(&s, "Rossi"), id),
            path(&["Prod", "Replica"])
        );
        // The password the file does not bring stays; the one it brings replaces.
        assert!(c.save_password);
        assert_eq!(s.password(id).unwrap(), Some(Secret::new("old-pass")));
        assert_eq!(
            s.ssh_secret(hops.secret_id).unwrap(),
            Some(Secret::new("new-ssh"))
        );
        // The dropped jump host's secret goes.
        assert_eq!(s.ssh_secret(jump_secret).unwrap(), None);
    }

    #[test]
    fn import_refuses_to_replace_an_unknown_connection() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path());
        let project = imported("Rossi");
        let a = project.connections[0].id;
        let result = s.apply_import(ImportRequest {
            targets: vec![new_project(project)],
            rail_folder: None,
            passwords: false,
            replace: BTreeMap::from([(a, Uuid::new_v4())]),
        });
        assert!(matches!(result, Err(StoreError::NotFound(_))));
        assert_eq!(s.list(), []);
    }

    #[test]
    fn import_saves_passwords_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(MemorySecrets::default());
        let mut s =
            ConnectionStore::open(dir.path().join("connections.toml"), secrets.clone()).unwrap();
        let project = with_secrets();
        let (a, b) = (project.connections[0].id, project.connections[1].id);
        let tunnel = project.connections[0].ssh.clone().unwrap();
        s.apply_import(ImportRequest {
            targets: vec![new_project(project)],
            rail_folder: None,
            passwords: true,
            replace: BTreeMap::new(),
        })
        .unwrap();
        assert!(s.get(a).unwrap().save_password);
        assert_eq!(s.password(a).unwrap(), Some(Secret::new("db-pass")));
        assert_eq!(
            s.ssh_secret(tunnel.secret_id).unwrap(),
            Some(Secret::new("ssh-pass"))
        );
        let jump = tunnel.jump.unwrap().secret_id;
        assert_eq!(s.ssh_secret(jump).unwrap(), Some(Secret::new("jump-pass")));
        // Without a saved password it asks on first use.
        assert!(!s.get(b).unwrap().save_password);
        // The file holds no password.
        let text = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        assert!(!text.contains("db-pass") && !text.contains("ssh-pass"));
    }

    #[test]
    fn import_without_passwords_saves_none() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(MemorySecrets::default());
        let mut s =
            ConnectionStore::open(dir.path().join("connections.toml"), secrets.clone()).unwrap();
        let project = with_secrets();
        let a = project.connections[0].id;
        let tunnel = project.connections[0].ssh.as_ref().unwrap().secret_id;
        s.apply_import(ImportRequest {
            targets: vec![new_project(project)],
            rail_folder: None,
            passwords: false,
            replace: BTreeMap::new(),
        })
        .unwrap();
        assert!(s.list().iter().all(|c| !c.save_password));
        assert_eq!(secrets.get(a).unwrap(), None);
        assert_eq!(secrets.get(tunnel).unwrap(), None);
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
            passwords: false,
            replace: BTreeMap::new(),
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
            passwords: false,
            replace: BTreeMap::new(),
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
            passwords: false,
            replace: BTreeMap::new(),
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
            passwords: false,
            replace: BTreeMap::new(),
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
            passwords: false,
            replace: BTreeMap::new(),
        });
        assert!(matches!(result, Err(StoreError::NotFound(_))));
        assert_eq!(s.list(), []);
        assert_eq!(s.projects(), []);
        assert!(!dir.path().join("connections.toml").exists());
    }
}
