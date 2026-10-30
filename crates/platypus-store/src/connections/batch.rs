//! Applying many changes at once, for synchronisation and shared projects: the caller has
//! already decided what the connections file should hold, and the store only has to take it
//! in, keep it consistent and write it once.

use uuid::Uuid;

use super::{
    ConnectionStore, NetworkProfile, Project, ProjectFolder, SavedConnection, SavedSsh, keep_known,
};
use crate::Result;

/// Changes to the connections file. Items are inserted or replaced by id; removals come after.
#[derive(Debug, Default)]
pub struct SyncBatch {
    pub network_profiles: Vec<NetworkProfile>,
    pub connections: Vec<SavedConnection>,
    pub projects: Vec<Project>,
    pub project_folders: Vec<ProjectFolder>,
    pub remove_network_profiles: Vec<Uuid>,
    pub remove_connections: Vec<Uuid>,
    pub remove_projects: Vec<Uuid>,
    pub remove_project_folders: Vec<Uuid>,
}

impl SyncBatch {
    /// Whether the batch changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.network_profiles.is_empty()
            && self.connections.is_empty()
            && self.projects.is_empty()
            && self.project_folders.is_empty()
            && self.remove_network_profiles.is_empty()
            && self.remove_connections.is_empty()
            && self.remove_projects.is_empty()
            && self.remove_project_folders.is_empty()
    }
}

/// Inserts `item` in place of the one with the same id, or at the end.
fn upsert<T>(list: &mut Vec<T>, item: T, id: impl Fn(&T) -> Uuid) {
    let wanted = id(&item);
    match list.iter().position(|x| id(x) == wanted) {
        Some(i) => list[i] = item,
        None => list.push(item),
    }
}

impl ConnectionStore {
    /// Applies `batch` and writes the file once. Passwords are never touched except those of
    /// removed connections; references to things that do not exist (a project's connection, a
    /// rail folder's project) are dropped, and a project whose name another project has gets
    /// a number after it.
    pub fn apply_batch(&mut self, batch: SyncBatch) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        let SyncBatch {
            network_profiles,
            connections,
            projects,
            project_folders,
            remove_network_profiles,
            remove_connections,
            remove_projects,
            remove_project_folders,
        } = batch;
        for p in network_profiles {
            upsert(&mut self.network_profiles, p, |p| p.id);
        }
        for c in connections {
            upsert(&mut self.connections, c, |c| c.id);
        }
        for p in projects {
            let at = self.projects.iter().position(|x| x.id == p.id);
            let name = self.unique_project_name(&p.name, p.id);
            let mut p = p;
            p.name = name;
            match at {
                Some(i) => self.projects[i] = p,
                None => self.projects.push(p),
            }
        }
        for f in project_folders {
            upsert(&mut self.project_folders, f, |f| f.id);
        }
        let removed_secrets = self.remove_batch(
            &remove_network_profiles,
            &remove_connections,
            &remove_projects,
            &remove_project_folders,
        );
        self.tidy();
        self.write()?;
        for id in removed_secrets {
            self.secrets.delete(id)?;
        }
        Ok(())
    }

    /// The name, or the name and a number, that no other project has (ignoring case).
    fn unique_project_name(&self, name: &str, id: Uuid) -> String {
        let taken = |n: &str| {
            let lower = n.to_lowercase();
            self.projects
                .iter()
                .any(|p| p.id != id && p.name.to_lowercase() == lower)
        };
        let mut candidate = name.to_owned();
        let mut n = 2;
        while taken(&candidate) {
            candidate = format!("{name} {n}");
            n += 1;
        }
        candidate
    }

    /// Removes what the batch deletes; returns the secret ids of the removed connections.
    fn remove_batch(
        &mut self,
        profiles: &[Uuid],
        connections: &[Uuid],
        projects: &[Uuid],
        folders: &[Uuid],
    ) -> Vec<Uuid> {
        let mut secrets = Vec::new();
        self.connections.retain(|c| {
            if !connections.contains(&c.id) {
                return true;
            }
            secrets.push(c.id);
            secrets.extend(c.ssh.iter().flat_map(SavedSsh::chain).map(|s| s.secret_id));
            false
        });
        self.network_profiles.retain(|p| !profiles.contains(&p.id));
        // A connection that used a removed profile keeps the settings the profile gave it.
        let kept: Vec<NetworkProfile> = self.network_profiles.clone();
        for c in &mut self.connections {
            if c.network_profile
                .is_some_and(|id| !kept.iter().any(|p| p.id == id))
            {
                c.network_profile = None;
            }
        }
        self.projects.retain(|p| !projects.contains(&p.id));
        self.project_folders.retain(|f| !folders.contains(&f.id));
        secrets
    }

    /// Makes the references consistent after a batch.
    fn tidy(&mut self) {
        let known = self.connections.iter().map(|c| c.id).collect();
        for p in &mut self.projects {
            keep_known(&mut p.connections, &known);
            p.drop_dangling();
        }
        let mut seen = std::collections::HashSet::new();
        for f in &mut self.project_folders {
            f.projects
                .retain(|id| self.projects.iter().any(|p| p.id == *id) && seen.insert(*id));
        }
        self.project_folders.retain(|f| !f.projects.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use platypus_core::DriverKind;

    use super::*;
    use crate::MemorySecrets;

    #[test]
    fn applies_upserts_and_removals_in_one_write() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ConnectionStore::open(
            dir.path().join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap();
        let a = SavedConnection::new("a", DriverKind::Sqlite);
        let b = SavedConnection::new("b", DriverKind::Sqlite);
        let mut p = Project::new("P", "#112233");
        p.connections = vec![a.id, b.id, Uuid::new_v4()];
        let other = Project::new("p", "#112233");
        s.apply_batch(SyncBatch {
            connections: vec![a.clone(), b.clone()],
            projects: vec![p.clone(), other],
            ..SyncBatch::default()
        })
        .unwrap();
        assert_eq!(s.revision(), 1);
        assert_eq!(s.list().len(), 2);
        assert_eq!(
            s.projects()[0].connections,
            [a.id, b.id],
            "unknown id dropped"
        );
        assert_eq!(s.projects()[1].name, "p 2", "name made unique");
        s.apply_batch(SyncBatch {
            remove_connections: vec![a.id],
            remove_projects: vec![p.id],
            ..SyncBatch::default()
        })
        .unwrap();
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.projects().len(), 1);
        assert_eq!(s.revision(), 2);
        s.apply_batch(SyncBatch::default()).unwrap();
        assert_eq!(s.revision(), 2, "an empty batch writes nothing");
    }
}
