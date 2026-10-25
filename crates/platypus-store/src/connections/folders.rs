//! Folders: of connections inside a project (nested, shown as a tree in the navigator), and
//! of projects in the rail (one level, like server folders in Discord).

use uuid::Uuid;

use super::{ConnectionFolder, ConnectionStore, Project, ProjectFolder, is_color, trim};
use crate::{Result, StoreError};

impl Project {
    fn folder(&self, id: Uuid) -> Option<&ConnectionFolder> {
        self.folders.iter().find(|f| f.id == id)
    }

    fn require_folder(&self, id: Uuid) -> Result<()> {
        self.folder(id).map(|_| ()).ok_or(StoreError::NotFound(id))
    }

    /// Whether `folder` is `ancestor` or inside it.
    fn is_within(&self, folder: Uuid, ancestor: Uuid) -> bool {
        let mut at = Some(folder);
        // Bounded by the number of folders, in case a file edited by hand has a cycle.
        for _ in 0..=self.folders.len() {
            match at {
                Some(id) if id == ancestor => return true,
                Some(id) => at = self.folder(id).and_then(|f| f.parent),
                None => return false,
            }
        }
        true
    }

    /// Refuses an empty name, or one used by another folder in `parent` (ignoring case).
    fn check_folder_name(
        &self,
        name: &str,
        parent: Option<Uuid>,
        except: Option<Uuid>,
    ) -> Result<()> {
        if name.is_empty() {
            return Err(StoreError::Invalid("folder name is required".to_owned()));
        }
        let lower = name.to_lowercase();
        if self
            .folders
            .iter()
            .any(|f| Some(f.id) != except && f.parent == parent && f.name.to_lowercase() == lower)
        {
            return Err(StoreError::Invalid(format!(
                "a folder named \"{name}\" exists already here"
            )));
        }
        Ok(())
    }

    /// The folder at `path` (names from the root, matched ignoring case), created where
    /// missing; `None` for an empty path.
    pub fn folder_path<S: AsRef<str>>(&mut self, path: &[S]) -> Option<Uuid> {
        let mut parent = None;
        for name in path {
            let name = name.as_ref();
            let lower = name.to_lowercase();
            let found = self
                .folders
                .iter()
                .find(|f| f.parent == parent && f.name.to_lowercase() == lower)
                .map(|f| f.id);
            parent = Some(found.unwrap_or_else(|| {
                let id = Uuid::new_v4();
                self.folders.push(ConnectionFolder {
                    id,
                    name: name.to_owned(),
                    parent,
                });
                id
            }));
        }
        parent
    }

    /// Fixes what a file edited by hand may hold: folders under a missing parent or in a
    /// cycle go to the root; placements of other connections or missing folders are dropped.
    pub(super) fn drop_dangling(&mut self) {
        for i in 0..self.folders.len() {
            let (id, parent) = (self.folders[i].id, self.folders[i].parent);
            if let Some(parent) = parent
                && (self.folder(parent).is_none() || self.is_within(parent, id))
            {
                self.folders[i].parent = None;
            }
        }
        let folders: Vec<Uuid> = self.folders.iter().map(|f| f.id).collect();
        let connections = &self.connections;
        self.placement
            .retain(|c, f| connections.contains(c) && folders.contains(f));
    }
}

impl ConnectionStore {
    fn project_mut(&mut self, id: Uuid) -> Result<&mut Project> {
        self.projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(StoreError::NotFound(id))
    }

    fn folder_of(&mut self, project: Uuid, folder: Uuid) -> Result<(&mut Project, usize)> {
        let p = self.project_mut(project)?;
        let index = p
            .folders
            .iter()
            .position(|f| f.id == folder)
            .ok_or(StoreError::NotFound(folder))?;
        Ok((p, index))
    }

    /// Creates a folder of connections in a project, at the root or inside `parent`.
    pub fn create_folder(
        &mut self,
        project: Uuid,
        name: &str,
        parent: Option<Uuid>,
    ) -> Result<Uuid> {
        let p = self.project_mut(project)?;
        let name = name.trim().to_owned();
        if let Some(parent) = parent {
            p.require_folder(parent)?;
        }
        p.check_folder_name(&name, parent, None)?;
        let id = Uuid::new_v4();
        p.folders.push(ConnectionFolder { id, name, parent });
        self.write()?;
        Ok(id)
    }

    pub fn rename_folder(&mut self, project: Uuid, folder: Uuid, name: &str) -> Result<()> {
        let (p, index) = self.folder_of(project, folder)?;
        let name = name.trim().to_owned();
        p.check_folder_name(&name, p.folders[index].parent, Some(folder))?;
        p.folders[index].name = name;
        self.write()
    }

    /// Moves a folder, with what it holds, to the root or into `parent`; not into itself.
    pub fn move_folder(&mut self, project: Uuid, folder: Uuid, parent: Option<Uuid>) -> Result<()> {
        let (p, index) = self.folder_of(project, folder)?;
        if let Some(parent) = parent {
            p.require_folder(parent)?;
            if p.is_within(parent, folder) {
                return Err(StoreError::Invalid(
                    "a folder cannot go inside itself".to_owned(),
                ));
            }
        }
        p.check_folder_name(&p.folders[index].name, parent, Some(folder))?;
        p.folders[index].parent = parent;
        self.write()
    }

    /// Deletes a folder: its subfolders and connections move up to its parent. A subfolder
    /// whose name is taken there gets a number after it.
    pub fn delete_folder(&mut self, project: Uuid, folder: Uuid) -> Result<()> {
        let (p, index) = self.folder_of(project, folder)?;
        let parent = p.folders.remove(index).parent;
        for i in 0..p.folders.len() {
            if p.folders[i].parent != Some(folder) {
                continue;
            }
            p.folders[i].parent = parent;
            let (id, name) = (p.folders[i].id, p.folders[i].name.clone());
            let mut n = 2;
            while p
                .check_folder_name(&p.folders[i].name, parent, Some(id))
                .is_err()
            {
                p.folders[i].name = format!("{name} ({n})");
                n += 1;
            }
        }
        match parent {
            Some(parent) => p
                .placement
                .values_mut()
                .filter(|f| **f == folder)
                .for_each(|f| *f = parent),
            None => p.placement.retain(|_, f| *f != folder),
        }
        self.write()
    }

    /// Puts a connection of the project into a folder, or (`None`) at the root.
    pub fn place_connection(
        &mut self,
        project: Uuid,
        connection: Uuid,
        folder: Option<Uuid>,
    ) -> Result<()> {
        let p = self.project_mut(project)?;
        if !p.connections.contains(&connection) {
            return Err(StoreError::NotFound(connection));
        }
        match folder {
            Some(f) => {
                p.require_folder(f)?;
                p.placement.insert(connection, f);
            }
            None => {
                p.placement.remove(&connection);
            }
        }
        self.write()
    }
}

/// Rail folders.
impl ConnectionStore {
    #[must_use]
    pub fn project_folders(&self) -> &[ProjectFolder] {
        &self.project_folders
    }

    pub(super) fn require_project(&self, id: Uuid) -> Result<()> {
        if self.projects.iter().any(|p| p.id == id) {
            Ok(())
        } else {
            Err(StoreError::NotFound(id))
        }
    }

    /// Takes a project out of its folder, if any; a folder left empty goes away.
    pub(super) fn leave_project_folder(&mut self, project: Uuid) {
        for f in &mut self.project_folders {
            f.projects.retain(|p| *p != project);
        }
        self.project_folders.retain(|f| !f.projects.is_empty());
    }

    /// Makes a rail folder holding `projects`, in the color of the first one.
    pub(super) fn add_project_folder(&mut self, name: String, projects: Vec<Uuid>) -> Uuid {
        let color = projects
            .first()
            .and_then(|first| self.projects.iter().find(|p| p.id == *first))
            .map(|p| p.color.clone())
            .unwrap_or_default();
        let id = Uuid::new_v4();
        self.project_folders.push(ProjectFolder {
            id,
            name,
            color,
            projects,
            collapsed: false,
        });
        id
    }

    /// Drops `dragged` onto `target`: it joins `target`'s folder, or a new folder with both
    /// is made, named "New folder" in `target`'s color. Returns the folder.
    pub fn group_projects(&mut self, target: Uuid, dragged: Uuid) -> Result<Uuid> {
        self.require_project(target)?;
        self.require_project(dragged)?;
        if target == dragged {
            return Err(StoreError::Invalid(
                "a project cannot be grouped with itself".to_owned(),
            ));
        }
        self.leave_project_folder(dragged);
        let id = if let Some(f) = self
            .project_folders
            .iter_mut()
            .find(|f| f.projects.contains(&target))
        {
            f.projects.push(dragged);
            f.id
        } else {
            self.add_project_folder("New folder".to_owned(), vec![target, dragged])
        };
        self.write()?;
        Ok(id)
    }

    /// Changes a folder's name, color or collapsed state; its projects stay as they are.
    pub fn save_project_folder(&mut self, mut folder: ProjectFolder) -> Result<()> {
        trim(&mut folder.name);
        if folder.name.is_empty() {
            return Err(StoreError::Invalid("folder name is required".to_owned()));
        }
        if !is_color(&folder.color) {
            return Err(StoreError::Invalid(format!(
                "invalid folder color `{}`",
                folder.color
            )));
        }
        let existing = self
            .project_folders
            .iter_mut()
            .find(|f| f.id == folder.id)
            .ok_or(StoreError::NotFound(folder.id))?;
        existing.name = folder.name;
        existing.color = folder.color;
        existing.collapsed = folder.collapsed;
        self.write()
    }

    /// Moves a project into a folder, at `index` (the end when `None` or past it), or out of
    /// any folder.
    pub fn set_project_folder(
        &mut self,
        project: Uuid,
        folder: Option<Uuid>,
        index: Option<usize>,
    ) -> Result<()> {
        self.require_project(project)?;
        if let Some(folder) = folder
            && !self.project_folders.iter().any(|f| f.id == folder)
        {
            return Err(StoreError::NotFound(folder));
        }
        // Within its own folder it only moves: leaving first could empty the folder away.
        let own = folder.is_some_and(|id| {
            self.project_folders
                .iter()
                .any(|f| f.id == id && f.projects.contains(&project))
        });
        if own {
            for f in &mut self.project_folders {
                f.projects.retain(|p| *p != project);
            }
        } else {
            self.leave_project_folder(project);
        }
        if let Some(f) = folder.and_then(|id| self.project_folders.iter_mut().find(|f| f.id == id))
        {
            let at = index.map_or(f.projects.len(), |i| i.min(f.projects.len()));
            f.projects.insert(at, project);
        }
        self.write()
    }

    /// Removes a folder; its projects stay in the rail.
    pub fn ungroup(&mut self, folder: Uuid) -> Result<()> {
        let before = self.project_folders.len();
        self.project_folders.retain(|f| f.id != folder);
        if self.project_folders.len() == before {
            return Err(StoreError::NotFound(folder));
        }
        self.write()
    }

    /// Puts the projects in this order, which must hold each of them once.
    pub fn reorder_projects(&mut self, order: &[Uuid]) -> Result<()> {
        let unique: std::collections::HashSet<_> = order.iter().collect();
        if order.len() != self.projects.len()
            || unique.len() != order.len()
            || self.projects.iter().any(|p| !unique.contains(&p.id))
        {
            return Err(StoreError::Invalid(
                "the new order must list every project once".to_owned(),
            ));
        }
        self.projects
            .sort_by_key(|p| order.iter().position(|id| *id == p.id));
        self.write()
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use platypus_core::DriverKind;
    use uuid::Uuid;

    use super::super::{ConnectionStore, PasswordUpdate, Project, ProjectFolder, SavedConnection};
    use crate::{MemorySecrets, StoreError};

    fn store(dir: &Path) -> ConnectionStore {
        ConnectionStore::open(
            dir.join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap()
    }

    /// A store with one project holding two connections.
    fn setup(dir: &Path) -> (ConnectionStore, Uuid, Uuid, Uuid) {
        let mut s = store(dir);
        let a = SavedConnection::new("a", DriverKind::Sqlite);
        let b = SavedConnection::new("b", DriverKind::Sqlite);
        let (a_id, b_id) = (a.id, b.id);
        s.save(a, PasswordUpdate::Keep).unwrap();
        s.save(b, PasswordUpdate::Keep).unwrap();
        let p = Project::new("P", "#d08f20");
        let p_id = p.id;
        s.save_project(p).unwrap();
        s.add_to_project(p_id, &[a_id, b_id]).unwrap();
        (s, p_id, a_id, b_id)
    }

    fn project(s: &ConnectionStore, id: Uuid) -> &Project {
        s.projects().iter().find(|p| p.id == id).unwrap()
    }

    fn invalid<T: std::fmt::Debug>(r: &crate::Result<T>) -> bool {
        matches!(r, Err(StoreError::Invalid(_)))
    }

    #[test]
    fn create_rename_delete_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, p, a, _) = setup(dir.path());
        assert!(invalid(&s.create_folder(p, "  ", None)));
        let prod = s.create_folder(p, "Prod", None).unwrap();
        assert!(invalid(&s.create_folder(p, "prod", None)));
        let replica = s.create_folder(p, "Replica", Some(prod)).unwrap();
        // The same name under another parent is fine.
        s.create_folder(p, "Prod", Some(replica)).unwrap();
        s.rename_folder(p, replica, "Replicas").unwrap();
        assert!(invalid(&s.rename_folder(p, replica, "")));
        s.place_connection(p, a, Some(replica)).unwrap();

        s.delete_folder(p, replica).unwrap();
        let pr = project(&s, p);
        assert_eq!(pr.folders.len(), 2);
        // The child and the connection move up to the deleted folder's parent.
        assert!(
            pr.folders
                .iter()
                .all(|f| f.id == prod || f.parent == Some(prod))
        );
        assert_eq!(pr.placement.get(&a), Some(&prod));

        // Reopened from disk: the same.
        let again = store(dir.path());
        assert_eq!(project(&again, p), project(&s, p));
    }

    #[test]
    fn move_folder_rejects_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, p, _, _) = setup(dir.path());
        let a = s.create_folder(p, "A", None).unwrap();
        let b = s.create_folder(p, "B", Some(a)).unwrap();
        assert!(invalid(&s.move_folder(p, a, Some(b))));
        assert!(invalid(&s.move_folder(p, a, Some(a))));
        s.move_folder(p, b, None).unwrap();
        assert_eq!(project(&s, p).folders[1].parent, None);
        // A name taken in the new parent is refused.
        s.create_folder(p, "X", None).unwrap();
        let x = s.create_folder(p, "X", Some(a)).unwrap();
        assert!(invalid(&s.move_folder(p, x, None)));
    }

    #[test]
    fn place_requires_membership() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, p, a, _) = setup(dir.path());
        let f = s.create_folder(p, "F", None).unwrap();
        let outside = SavedConnection::new("c", DriverKind::Sqlite);
        let outside_id = outside.id;
        s.save(outside, PasswordUpdate::Keep).unwrap();
        assert!(matches!(
            s.place_connection(p, outside_id, Some(f)),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            s.place_connection(p, a, Some(Uuid::new_v4())),
            Err(StoreError::NotFound(_))
        ));
        s.place_connection(p, a, Some(f)).unwrap();
        s.place_connection(p, a, None).unwrap();
        assert!(project(&s, p).placement.is_empty());
    }

    #[test]
    fn removing_cleans_placement() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, p, a, b) = setup(dir.path());
        let f = s.create_folder(p, "F", None).unwrap();
        s.place_connection(p, a, Some(f)).unwrap();
        s.place_connection(p, b, Some(f)).unwrap();
        s.set_membership(a, p, false).unwrap();
        s.delete(b).unwrap();
        assert!(project(&s, p).placement.is_empty());
    }

    #[test]
    fn saving_a_project_keeps_its_folders() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, p, a, _) = setup(dir.path());
        let f = s.create_folder(p, "F", None).unwrap();
        s.place_connection(p, a, Some(f)).unwrap();
        // The project dialog sends the project without its folders.
        let mut edited = project(&s, p).clone();
        edited.name = "Renamed".to_owned();
        edited.folders.clear();
        edited.placement.clear();
        s.save_project(edited).unwrap();
        let pr = project(&s, p);
        assert_eq!(pr.name, "Renamed");
        assert_eq!(pr.folders.len(), 1);
        assert_eq!(pr.placement.get(&a), Some(&f));
    }

    #[test]
    fn folder_path_creates_missing() {
        let mut p = Project::new("P", "#d08f20");
        let first = p.folder_path(&["A", "B"]);
        let second = p.folder_path(&["a", "B"]);
        assert!(first.is_some());
        assert_eq!(first, second);
        assert_eq!(p.folder_path::<&str>(&[]), None);
        assert_eq!(p.folders.len(), 2);
        let other = p.folder_path(&["A", "C"]);
        assert_ne!(other, first);
        assert_eq!(p.folders.len(), 3);
    }

    #[test]
    fn v4_file_reads_and_writes_v5() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.toml");
        let (p, c) = (Uuid::new_v4(), Uuid::new_v4());
        std::fs::write(
            &path,
            format!(
                "version = 4\n\n[[projects]]\nid = \"{p}\"\nname = \"P\"\ncolor = \"#d08f20\"\n\
                 connections = [\"{c}\"]\n\n[[connections]]\nid = \"{c}\"\nname = \"c\"\n\
                 driver = \"sqlite\"\ndatabase = 'x.db'\n"
            ),
        )
        .unwrap();
        let mut s = store(dir.path());
        assert_eq!(project(&s, p).folders, []);
        s.create_folder(p, "F", None).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("version = 5"), "{text}");
    }

    #[test]
    fn open_drops_dangling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.toml");
        let (p, c, f, orphan) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (gone, gone_folder) = (Uuid::new_v4(), Uuid::new_v4());
        std::fs::write(
            &path,
            format!(
                "version = 5\n\n[[projects]]\nid = \"{p}\"\nname = \"P\"\ncolor = \"#d08f20\"\n\
                 connections = [\"{c}\"]\n\n[[projects.folders]]\nid = \"{f}\"\nname = \"F\"\n\n\
                 [[projects.folders]]\nid = \"{orphan}\"\nname = \"O\"\nparent = \"{gone_folder}\"\n\n\
                 [projects.placement]\n\"{c}\" = \"{gone_folder}\"\n\"{gone}\" = \"{f}\"\n\n\
                 [[connections]]\nid = \"{c}\"\nname = \"c\"\ndriver = \"sqlite\"\ndatabase = 'x.db'\n"
            ),
        )
        .unwrap();
        let s = store(dir.path());
        let pr = project(&s, p);
        assert!(pr.placement.is_empty(), "{:?}", pr.placement);
        assert_eq!(pr.folders.len(), 2);
        assert!(pr.folders.iter().all(|f| f.parent.is_none()));
    }

    /// A store with `n` empty projects, in order.
    fn projects(dir: &Path, n: usize) -> (ConnectionStore, Vec<Uuid>) {
        let mut s = store(dir);
        let ids = (0..n)
            .map(|i| {
                let p = Project::new(&format!("P{i}"), "#d08f20");
                let id = p.id;
                s.save_project(p).unwrap();
                id
            })
            .collect();
        (s, ids)
    }

    fn folder(s: &ConnectionStore, id: Uuid) -> &ProjectFolder {
        s.project_folders().iter().find(|f| f.id == id).unwrap()
    }

    #[test]
    fn group_creates_then_extends() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, ids) = projects(dir.path(), 3);
        let f = s.group_projects(ids[0], ids[1]).unwrap();
        assert_eq!(folder(&s, f).projects, [ids[0], ids[1]]);
        assert_eq!(folder(&s, f).name, "New folder");
        assert_eq!(folder(&s, f).color, "#d08f20");
        // Onto a project already in a folder: that folder grows.
        assert_eq!(s.group_projects(ids[1], ids[2]).unwrap(), f);
        assert_eq!(folder(&s, f).projects, ids);
        assert!(invalid(&s.group_projects(ids[0], ids[0])));
    }

    #[test]
    fn moving_into_its_own_folder_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, ids) = projects(dir.path(), 2);
        let f = s.group_projects(ids[0], ids[1]).unwrap();
        s.set_project_folder(ids[1], None, None).unwrap();
        // The last project dropped into its own folder: the folder stays.
        s.set_project_folder(ids[0], Some(f), None).unwrap();
        assert_eq!(folder(&s, f).projects, [ids[0]]);
    }

    #[test]
    fn project_in_one_folder_only() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, ids) = projects(dir.path(), 4);
        let f = s.group_projects(ids[0], ids[1]).unwrap();
        let g = s.group_projects(ids[2], ids[3]).unwrap();
        s.set_project_folder(ids[1], Some(g), Some(0)).unwrap();
        assert_eq!(folder(&s, f).projects, [ids[0]]);
        assert_eq!(folder(&s, g).projects, [ids[1], ids[2], ids[3]]);
        s.set_project_folder(ids[2], None, None).unwrap();
        assert_eq!(folder(&s, g).projects, [ids[1], ids[3]]);
    }

    #[test]
    fn empty_folder_disappears() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, ids) = projects(dir.path(), 3);
        let f = s.group_projects(ids[0], ids[1]).unwrap();
        s.set_project_folder(ids[0], None, None).unwrap();
        s.delete_project(ids[1]).unwrap();
        assert_eq!(s.project_folders(), []);
        let g = s.group_projects(ids[0], ids[2]).unwrap();
        assert_ne!(f, g);
        s.ungroup(g).unwrap();
        assert_eq!(s.project_folders(), []);
        assert_eq!(s.projects().len(), 2);
    }

    #[test]
    fn reorder_must_be_permutation() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, ids) = projects(dir.path(), 3);
        assert!(invalid(&s.reorder_projects(&[ids[0], ids[1]])));
        assert!(invalid(&s.reorder_projects(&[ids[0], ids[1], ids[1]])));
        s.reorder_projects(&[ids[2], ids[0], ids[1]]).unwrap();
        let order: Vec<Uuid> = s.projects().iter().map(|p| p.id).collect();
        assert_eq!(order, [ids[2], ids[0], ids[1]]);
    }

    #[test]
    fn project_folders_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, ids) = projects(dir.path(), 2);
        let f = s.group_projects(ids[0], ids[1]).unwrap();
        let mut edited = folder(&s, f).clone();
        edited.name = "Clients".to_owned();
        edited.color = "#3d7bf7".to_owned();
        edited.collapsed = true;
        s.save_project_folder(edited.clone()).unwrap();
        let mut bad = edited.clone();
        bad.color = "blue".to_owned();
        assert!(invalid(&s.save_project_folder(bad)));
        bad = edited.clone();
        bad.name = " ".to_owned();
        assert!(invalid(&s.save_project_folder(bad)));
        let again = store(dir.path());
        assert_eq!(again.project_folders(), [edited]);
    }
}
