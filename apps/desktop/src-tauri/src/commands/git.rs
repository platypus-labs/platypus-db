//! Git for the saved scripts: the commands of the Git bar of the Scripts panel. They work on
//! the project's scripts folder, resolved as `scripts.rs` does, and run Git off the main
//! thread because a pull or a push may wait on the network.

use std::path::Path;

use platypus_sync::git::{self, FileDiff, GitError, Identity, PullOutcome, RepoStatus};
use tauri::State;
use uuid::Uuid;

use super::AppState;
use super::scripts::root;
use crate::error::{CommandError, CommandResult};

impl From<GitError> for CommandError {
    fn from(e: GitError) -> Self {
        Self::new(e.kind(), e.to_string())
    }
}

/// Runs `f` in the project's scripts folder on a blocking thread.
async fn in_scripts<T: Send + 'static>(
    state: &AppState,
    project: Option<Uuid>,
    f: impl FnOnce(&Path) -> Result<T, GitError> + Send + 'static,
) -> CommandResult<T> {
    let dir = root(state, project)?;
    tauri::async_runtime::spawn_blocking(move || f(&dir))
        .await
        .map_err(|e| CommandError::new("other", e.to_string()))?
        .map_err(CommandError::from)
}

/// The branch, how far it is from its upstream and the files changed; `is_repo` is false
/// when the folder is not a repository yet.
#[tauri::command]
pub async fn git_status(
    state: State<'_, AppState>,
    project: Option<Uuid>,
) -> CommandResult<RepoStatus> {
    in_scripts(&state, project, git::status).await
}

/// Turns the project's scripts folder into a Git repository.
#[tauri::command]
pub async fn git_init(state: State<'_, AppState>, project: Option<Uuid>) -> CommandResult<()> {
    in_scripts(&state, project, git::init).await
}

/// What changed in one script since the last commit.
#[tauri::command]
pub async fn git_diff(
    state: State<'_, AppState>,
    project: Option<Uuid>,
    path: String,
) -> CommandResult<FileDiff> {
    in_scripts(&state, project, move |d| git::diff(d, &path)).await
}

/// Commits the files `paths` with `message`; returns the short id of the commit. `identity`
/// is set in the repository when Git does not know the author (error `git_identity_missing`).
#[tauri::command]
pub async fn git_commit(
    state: State<'_, AppState>,
    project: Option<Uuid>,
    paths: Vec<String>,
    message: String,
    identity: Option<Identity>,
) -> CommandResult<String> {
    in_scripts(&state, project, move |d| {
        git::commit(d, &paths, &message, identity.as_ref())
    })
    .await
}

/// Fetches from the remote without changing any file.
#[tauri::command]
pub async fn git_fetch(state: State<'_, AppState>, project: Option<Uuid>) -> CommandResult<()> {
    in_scripts(&state, project, git::fetch).await
}

/// Fetches and fast-forwards; `diverged` when it cannot.
#[tauri::command]
pub async fn git_pull(
    state: State<'_, AppState>,
    project: Option<Uuid>,
) -> CommandResult<PullOutcome> {
    in_scripts(&state, project, git::pull).await
}

/// Pulls with a merge, for a divergence; may stop on conflicts.
#[tauri::command]
pub async fn git_pull_merge(
    state: State<'_, AppState>,
    project: Option<Uuid>,
    identity: Option<Identity>,
) -> CommandResult<PullOutcome> {
    in_scripts(&state, project, move |d| {
        git::pull_merge(d, identity.as_ref())
    })
    .await
}

/// Gives up a merge that stopped on conflicts.
#[tauri::command]
pub async fn git_merge_abort(
    state: State<'_, AppState>,
    project: Option<Uuid>,
) -> CommandResult<()> {
    in_scripts(&state, project, git::merge_abort).await
}

/// Pushes the branch, publishing it on the first remote if it has no upstream.
#[tauri::command]
pub async fn git_push(state: State<'_, AppState>, project: Option<Uuid>) -> CommandResult<()> {
    in_scripts(&state, project, git::push).await
}
