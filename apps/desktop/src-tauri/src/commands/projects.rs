//! Projects: sets of connections worked on together, kept in `connections.toml`. Every
//! command returns the projects as they are after the change, in rail order.

use platypus_store::Project;
use tauri::State;
use uuid::Uuid;

use super::AppState;
use crate::error::CommandResult;

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
