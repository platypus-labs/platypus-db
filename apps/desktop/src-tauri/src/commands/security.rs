//! The users, roles and privileges of the server, for the admin tab: what they are, and the
//! statements that change them. The statements are written here, from a description of the
//! change, so that a password is only ever in the statement that runs: the preview shows it
//! masked, and so does the query log.

use futures::TryStreamExt;
use platypus_core::{
    ExecOptions, ExecResult, Principal, PrincipalFlag, PrivilegeGrant, SecurityChange, TargetClass,
};
use platypus_sql::{Dialect, security};
use serde::Serialize;
use tauri::State;

use super::{AppState, one_off};
use crate::error::{CommandError, CommandResult};

/// The users and roles of the server.
#[tauri::command]
pub async fn list_principals(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<Vec<Principal>> {
    let conn = state.live(session_id).await?;
    Ok(conn.principals().await?)
}

/// The privileges given to a user or role itself.
#[tauri::command]
pub async fn principal_privileges(
    state: State<'_, AppState>,
    session_id: u64,
    name: String,
    host: Option<String>,
) -> CommandResult<Vec<PrivilegeGrant>> {
    let conn = state.live(session_id).await?;
    Ok(conn.principal_privileges(&name, host.as_deref()).await?)
}

/// The privileges a grant can name for one kind of object.
#[derive(Debug, Serialize)]
pub struct TargetPrivileges {
    target: TargetClass,
    privileges: &'static [&'static str],
}

/// What the form of a database offers.
#[derive(Debug, Serialize)]
pub struct SecurityOptions {
    /// The attributes a user or role has, in the order to show them.
    flags: &'static [PrincipalFlag],
    /// Whether the server has roles (MySQL before 8.0 has none).
    roles: bool,
    /// Privileges by kind of object; a kind with none cannot be granted on.
    targets: Vec<TargetPrivileges>,
}

const CLASSES: [TargetClass; 6] = [
    TargetClass::Server,
    TargetClass::Database,
    TargetClass::Schema,
    TargetClass::Table,
    TargetClass::AllTables,
    TargetClass::AllRoutines,
];

/// The attributes and the privileges of the session's database.
#[tauri::command]
pub async fn security_options(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<SecurityOptions> {
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    let conn = state.live(session_id).await?;
    Ok(SecurityOptions {
        flags: security::flags(dialect),
        roles: security::roles_supported(dialect, conn.server_info()),
        targets: CLASSES
            .into_iter()
            .map(|target| TargetPrivileges {
                target,
                privileges: security::privileges(dialect, target),
            })
            .collect(),
    })
}

/// The statements `change` runs, a password masked.
#[tauri::command]
pub async fn security_preview(
    state: State<'_, AppState>,
    session_id: u64,
    change: SecurityChange,
) -> CommandResult<Vec<String>> {
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    check_roles(&state, session_id, dialect, &change).await?;
    security::statements(dialect, &change, true)
        .map_err(|e| CommandError::new("invalid_security_change", e))
}

/// Runs `change` on the session, a statement at a time and each committed at once, and
/// returns the statements as they are shown (a password masked). Not on a read-only
/// connection.
#[tauri::command]
pub async fn apply_security(
    state: State<'_, AppState>,
    session_id: u64,
    change: SecurityChange,
) -> CommandResult<Vec<String>> {
    state.writable(session_id)?;
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    check_roles(&state, session_id, dialect, &change).await?;
    let statements = security::statements(dialect, &change, false)
        .map_err(|e| CommandError::new("invalid_security_change", e))?;
    let shown = security::statements(dialect, &change, true)
        .map_err(|e| CommandError::new("invalid_security_change", e))?;
    let conn = state.live(session_id).await?;
    one_off(&conn, async {
        for sql in &statements {
            match conn.execute(sql, &ExecOptions::default()).await? {
                ExecResult::Affected { .. } => {}
                ExecResult::Rows(stream) => stream.try_for_each(|_| async { Ok(()) }).await?,
            }
        }
        Ok(())
    })
    .await?;
    Ok(shown)
}

/// Refuses a change to roles on a server that has none.
async fn check_roles(
    state: &State<'_, AppState>,
    session_id: u64,
    dialect: Dialect,
    change: &SecurityChange,
) -> CommandResult<()> {
    if !security::involves_roles(change) {
        return Ok(());
    }
    let conn = state.live(session_id).await?;
    if security::roles_supported(dialect, conn.server_info()) {
        Ok(())
    } else {
        Err(CommandError::new(
            "invalid_security_change",
            "this server has no roles: they came with MySQL 8.0",
        ))
    }
}
