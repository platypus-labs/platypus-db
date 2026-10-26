// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    // `args_os`: an argument that is not Unicode (a file path on Linux) must not stop the app.
    let mut args = std::env::args_os().skip(1);
    // Agents start the app as their MCP server, talking over stdin and stdout: no window.
    if args.next().is_some_and(|a| a == platypus_desktop::MCP_FLAG) {
        let rest = args.map(|a| a.to_string_lossy().into_owned());
        return platypus_mcp::stdio::main("platypus-db --mcp", rest);
    }
    platypus_desktop::run();
    ExitCode::SUCCESS
}
