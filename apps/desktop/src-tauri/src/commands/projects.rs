//! Projects: sets of connections worked on together, kept in `connections.toml`. Every
//! command returns the projects as they are after the change, in rail order.

use platypus_store::{ConnectionStore, Project, ProjectFolder, SavedConnection};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::AppState;
use crate::error::CommandResult;

/// Connections, projects and rail folders, for changes that touch more than one.
#[derive(Debug, Serialize)]
pub struct Workspace {
    connections: Vec<SavedConnection>,
    projects: Vec<Project>,
    project_folders: Vec<ProjectFolder>,
}

impl Workspace {
    pub fn of(s: &ConnectionStore) -> Self {
        Self {
            connections: s.list().to_vec(),
            projects: s.projects().to_vec(),
            project_folders: s.project_folders().to_vec(),
        }
    }
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> CommandResult<Vec<Project>> {
    state.with_store(|s| Ok(s.projects().to_vec()))
}

/// Creates or changes a project (by id); see `ConnectionStore::save_project` for the checks.
#[tauri::command]
pub fn save_project(state: State<'_, AppState>, project: Project) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.save_project(project)?;
        Ok(s.projects().to_vec())
    })
}

/// Deletes a project; its connections stay.
#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: Uuid) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.delete_project(id)?;
        Ok(s.projects().to_vec())
    })
}

/// Adds a connection to a project, or (`member` false) takes it out.
#[tauri::command]
pub fn set_project_membership(
    state: State<'_, AppState>,
    connection_id: Uuid,
    project_id: Uuid,
    member: bool,
) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.set_membership(connection_id, project_id, member)?;
        Ok(s.projects().to_vec())
    })
}

/// The initials a project gets from its name, shown while it is typed.
#[tauri::command]
pub fn project_initials(name: String) -> String {
    platypus_store::initials_of(&name)
}

/// Creates a folder of connections in a project, at the root or inside `parent`.
#[tauri::command]
pub fn create_connection_folder(
    state: State<'_, AppState>,
    project_id: Uuid,
    name: String,
    parent: Option<Uuid>,
) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.create_folder(project_id, &name, parent)?;
        Ok(s.projects().to_vec())
    })
}

#[tauri::command]
pub fn rename_connection_folder(
    state: State<'_, AppState>,
    project_id: Uuid,
    folder_id: Uuid,
    name: String,
) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.rename_folder(project_id, folder_id, &name)?;
        Ok(s.projects().to_vec())
    })
}

/// Moves a folder to the root or into `parent`.
#[tauri::command]
pub fn move_connection_folder(
    state: State<'_, AppState>,
    project_id: Uuid,
    folder_id: Uuid,
    parent: Option<Uuid>,
) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.move_folder(project_id, folder_id, parent)?;
        Ok(s.projects().to_vec())
    })
}

/// Deletes a folder; what it holds moves up to its parent.
#[tauri::command]
pub fn delete_connection_folder(
    state: State<'_, AppState>,
    project_id: Uuid,
    folder_id: Uuid,
) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.delete_folder(project_id, folder_id)?;
        Ok(s.projects().to_vec())
    })
}

/// Puts a connection into a folder of the project, or (`None`) at its root.
#[tauri::command]
pub fn place_connection(
    state: State<'_, AppState>,
    project_id: Uuid,
    connection_id: Uuid,
    folder_id: Option<Uuid>,
) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.place_connection(project_id, connection_id, folder_id)?;
        Ok(s.projects().to_vec())
    })
}

/// Moves a connection into a folder of the project (`None`: its root), just before
/// `before` or after the last connection there.
#[tauri::command]
pub fn arrange_connection(
    state: State<'_, AppState>,
    project_id: Uuid,
    connection_id: Uuid,
    folder_id: Option<Uuid>,
    before: Option<Uuid>,
) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.arrange_connection(project_id, connection_id, folder_id, before)?;
        Ok(s.projects().to_vec())
    })
}

/// Moves a folder into `parent` (`None`: the root), just before the folder `before` or after
/// the last one there.
#[tauri::command]
pub fn arrange_connection_folder(
    state: State<'_, AppState>,
    project_id: Uuid,
    folder_id: Uuid,
    parent: Option<Uuid>,
    before: Option<Uuid>,
) -> CommandResult<Vec<Project>> {
    state.with_store(|s| {
        s.arrange_folder(project_id, folder_id, parent, before)?;
        Ok(s.projects().to_vec())
    })
}

/// The projects after a drop of a connection onto another, and the folder it made.
#[derive(Serialize)]
pub struct Grouped {
    folder: Uuid,
    projects: Vec<Project>,
}

/// Drops the connection `dragged` onto `target`: a new folder named `name` holds both.
#[tauri::command]
pub fn group_connections(
    state: State<'_, AppState>,
    project_id: Uuid,
    target: Uuid,
    dragged: Uuid,
    name: String,
) -> CommandResult<Grouped> {
    state.with_store(|s| {
        let folder = s.group_connections(project_id, target, dragged, &name)?;
        Ok(Grouped {
            folder,
            projects: s.projects().to_vec(),
        })
    })
}

/// Moves a connection in the list All connections shows: just before `before`, or to the end.
#[tauri::command]
pub fn reorder_connection(
    state: State<'_, AppState>,
    connection_id: Uuid,
    before: Option<Uuid>,
) -> CommandResult<Workspace> {
    state.with_store(|s| {
        s.reorder_connection(connection_id, before)?;
        Ok(Workspace::of(s))
    })
}

#[tauri::command]
pub fn list_project_folders(state: State<'_, AppState>) -> CommandResult<Vec<ProjectFolder>> {
    state.with_store(|s| Ok(s.project_folders().to_vec()))
}

/// Drops a project onto another in the rail: both end up in one folder.
#[tauri::command]
pub fn group_projects(
    state: State<'_, AppState>,
    target: Uuid,
    dragged: Uuid,
) -> CommandResult<Workspace> {
    state.with_store(|s| {
        s.group_projects(target, dragged)?;
        Ok(Workspace::of(s))
    })
}

/// Changes a rail folder's name, color or collapsed state.
#[tauri::command]
pub fn save_project_folder(
    state: State<'_, AppState>,
    folder: ProjectFolder,
) -> CommandResult<Workspace> {
    state.with_store(|s| {
        s.save_project_folder(folder)?;
        Ok(Workspace::of(s))
    })
}

/// Moves a project into a rail folder at `index`, or out of any.
#[tauri::command]
pub fn set_project_folder(
    state: State<'_, AppState>,
    project_id: Uuid,
    folder_id: Option<Uuid>,
    index: Option<usize>,
) -> CommandResult<Workspace> {
    state.with_store(|s| {
        s.set_project_folder(project_id, folder_id, index)?;
        Ok(Workspace::of(s))
    })
}

/// Removes a rail folder; its projects stay.
#[tauri::command]
pub fn ungroup_projects(state: State<'_, AppState>, folder_id: Uuid) -> CommandResult<Workspace> {
    state.with_store(|s| {
        s.ungroup(folder_id)?;
        Ok(Workspace::of(s))
    })
}

/// Puts the projects of the rail in this order.
#[tauri::command]
pub fn reorder_projects(state: State<'_, AppState>, order: Vec<Uuid>) -> CommandResult<Workspace> {
    state.with_store(|s| {
        s.reorder_projects(&order)?;
        Ok(Workspace::of(s))
    })
}
