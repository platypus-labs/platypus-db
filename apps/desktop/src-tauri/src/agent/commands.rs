//! The bridge in the app: started with it as Settings › Agents say, and the page's commands
//! to set it up and to answer the agents' changes.

// Tauri hands commands their state and arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::PathBuf;
use std::sync::Arc;

use platypus_mcp::{Agent, ContextVerdict, DiagramAnswer, OpenSession, Profile, Verdict};
use tauri::{AppHandle, Emitter, Manager, State};

use super::{
    Bridge, BridgeStatus, ConnectedAgent, DEFAULT_PORT, Host, Link, WaitingContext, WaitingWrite,
};
use crate::commands::AppState;

/// The settings of the bridge (`agents.ts` in the page).
const ENABLED_KEY: &str = "agents.bridge";
const PORT_KEY: &str = "agents.bridge_port";

struct TauriHost(AppHandle);

impl Host for TauriHost {
    fn sessions(&self) -> Vec<OpenSession> {
        self.0
            .try_state::<AppState>()
            .map(|state| state.open_sessions())
            .unwrap_or_default()
    }

    fn notify(&self, event: &str, payload: serde_json::Value) {
        let _ = self.0.emit(event, payload);
    }
}

/// Whether the bridge runs, and on which port.
fn configured(state: &AppState) -> (bool, u16) {
    let enabled = state.setting::<bool>(ENABLED_KEY).unwrap_or(true);
    let port = state
        .setting::<i64>(PORT_KEY)
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p > 0)
        .unwrap_or(DEFAULT_PORT);
    (enabled, port)
}

/// Starts the bridge with the app, writing its files in `data_dir`.
pub fn start(app: &AppHandle, data_dir: Option<PathBuf>) {
    let link = Arc::new(Link::new(TauriHost(app.clone())));
    app.manage(Bridge::new(link, data_dir));
    let app = app.clone();
    tauri::async_runtime::spawn(async move { restart(&app).await });
}

/// Stops the bridge as the app exits: agents see it closed.
pub fn stop(app: &AppHandle) {
    if let Some(bridge) = app.try_state::<Bridge>() {
        bridge.stop();
    }
}

async fn restart(app: &AppHandle) -> BridgeStatus {
    let (state, bridge) = (app.state::<AppState>(), app.state::<Bridge>());
    let (enabled, port) = configured(&state);
    // The app's own tools reach the user through the same link as the others.
    let agent = Profile::from_env()
        .map(|profile| Arc::new(Agent::new(profile).with_link(Arc::clone(bridge.link()) as _)))
        .map_err(|e| e.to_string());
    bridge.start(enabled, port, agent).await;
    bridge.status(enabled, port)
}

#[tauri::command]
pub fn agent_bridge(state: State<'_, AppState>, bridge: State<'_, Bridge>) -> BridgeStatus {
    let (enabled, port) = configured(&state);
    bridge.status(enabled, port)
}

/// Starts the bridge again after its settings changed.
#[tauri::command]
pub async fn restart_agent_bridge(app: AppHandle) -> BridgeStatus {
    restart(&app).await
}

/// A new token, the bridge started again with it.
#[tauri::command]
pub async fn regenerate_agent_token(app: AppHandle) -> Result<BridgeStatus, String> {
    app.state::<Bridge>().regenerate()?;
    Ok(restart(&app).await)
}

/// Answers the change `id` an agent asked for; `false` when it no longer waits.
#[tauri::command]
pub fn answer_agent_write(bridge: State<'_, Bridge>, id: String, verdict: Verdict) -> bool {
    bridge.link().answer(&id, verdict)
}

/// The changes waiting for an answer: the page asks at its start.
#[tauri::command]
pub fn agent_writes_waiting(bridge: State<'_, Bridge>) -> Vec<WaitingWrite> {
    bridge.link().waiting()
}

/// Answers the request `id` of an agent to see what the user works on; `false` when it no
/// longer waits.
#[tauri::command]
pub fn agent_answer_context(
    bridge: State<'_, Bridge>,
    id: String,
    verdict: ContextVerdict,
) -> bool {
    bridge.link().answer_context(&id, verdict)
}

/// The requests to see what the user works on that wait for an answer: the page asks at its
/// start.
#[tauri::command]
pub fn agent_contexts_waiting(bridge: State<'_, Bridge>) -> Vec<WaitingContext> {
    bridge.link().waiting_contexts()
}

/// Answers the diagram `id` an agent asked the page to draw; `false` when it no longer waits.
#[tauri::command]
pub fn agent_answer_diagram(bridge: State<'_, Bridge>, id: String, answer: DiagramAnswer) -> bool {
    bridge.link().answer_diagram(&id, answer)
}

#[tauri::command]
pub fn connected_agents(bridge: State<'_, Bridge>) -> Vec<ConnectedAgent> {
    bridge.link().connected()
}
