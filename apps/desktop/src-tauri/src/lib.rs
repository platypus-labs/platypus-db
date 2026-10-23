//! Tauri shell for PlatypusDB: exposes the core crates to the web UI as commands.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    os: &'static str,
    arch: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "PlatypusDB",
        version: platypus_core::VERSION,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start (e.g. missing WebView2 on Windows).
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("error while running PlatypusDB");
}
