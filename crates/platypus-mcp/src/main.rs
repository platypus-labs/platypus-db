//! `platypus-mcp`: PlatypusDB's MCP server. An agent (Claude Code, Codex, Cursor...)
//! starts it and talks to it over stdin and stdout; the app does not need to be open.
//!
//! ```text
//! claude mcp add platypus -- /path/to/platypus-mcp
//! ```
//!
//! It uses the connections of the app's profile, or of `PLATYPUS_DATA_DIR` when set.

mod server;

use std::process::ExitCode;
use std::sync::Arc;

use platypus_mcp::{Agent, Profile};
use rmcp::ServiceExt;

use crate::server::PlatypusServer;

const HELP: &str = "platypus-mcp: PlatypusDB's MCP server, for agents such as Claude Code or Codex.

It talks MCP over stdin and stdout and is started by the agent, for example:

    claude mcp add platypus -- <path to platypus-mcp>

Agents use the connections saved in PlatypusDB (or in PLATYPUS_DATA_DIR when set), with the
access each connection gives them in the app: off, read-only (the default) or read-write.";

#[tokio::main]
async fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("platypus-mcp {}", platypus_core::VERSION);
            return ExitCode::SUCCESS;
        }
        Some("--help" | "-h") => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        // Stdout carries the protocol: errors go to stderr, which clients log.
        Err(e) => {
            eprintln!("platypus-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let agent = Arc::new(Agent::new(Profile::from_env()?));
    let service = PlatypusServer::new(agent)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
