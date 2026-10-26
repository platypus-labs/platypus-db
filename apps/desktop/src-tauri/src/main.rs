// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    // Agents start the app as their MCP server, talking over stdin and stdout: no window.
    if args.next().as_deref() == Some(platypus_desktop::MCP_FLAG) {
        return platypus_mcp::stdio::main("platypus-db --mcp", args);
    }
    platypus_desktop::run();
    ExitCode::SUCCESS
}
