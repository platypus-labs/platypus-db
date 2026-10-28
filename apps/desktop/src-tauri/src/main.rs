// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    // `args_os`: an argument that is not Unicode (a file path on Linux) must not stop the app.
    let mut args = std::env::args_os().skip(1);
    // Agents start the app as their MCP server, talking over stdin and stdout: no window.
    if args.next().is_some_and(|a| a == platypus_desktop::MCP_FLAG) {
        #[cfg(windows)]
        attach_console();
        let rest = args.map(|a| a.to_string_lossy().into_owned());
        return platypus_mcp::stdio::main("platypus-db --mcp", rest);
    }
    platypus_desktop::run();
    ExitCode::SUCCESS
}

/// Started from a terminal, a Windows release build (a window app) has no standard output:
/// `--help`, `--version` and the errors would print nothing. It takes the terminal's
/// console then; the pipes an MCP client gives it are kept.
#[cfg(windows)]
#[allow(unsafe_code)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: neither call takes pointers; a failure (no parent console) leaves things as
    // they were.
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
