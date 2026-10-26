//! Serving MCP over stdin and stdout, as the agent that starts the server expects. The app
//! does it with `platypus-db --mcp`; the `platypus-mcp` binary is the same server, for
//! development and builds from source.

use std::process::ExitCode;
use std::sync::Arc;

use rmcp::ServiceExt;

use crate::server::PlatypusServer;
use crate::{Agent, Profile};

/// What `--help` prints; `{command}` is how the agent starts the server.
const HELP: &str = "{command}: PlatypusDB's MCP server, for agents such as Claude Code or Codex.

It talks MCP over stdin and stdout and is started by the agent, for example:

    claude mcp add platypus -- {command}

Agents use the connections saved in PlatypusDB (or in PLATYPUS_DATA_DIR when set), with the
access each connection gives them in the app: off, read-only (the default) or read-write.";

/// Runs the server until the agent closes stdin. `command` names the server in `--help`,
/// `--version` and errors; `args` are the arguments after it, which may ask for those two.
#[must_use]
pub fn main(command: &str, mut args: impl Iterator<Item = String>) -> ExitCode {
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("{command} {}", platypus_core::VERSION);
            return ExitCode::SUCCESS;
        }
        Some("--help" | "-h") => {
            println!("{}", HELP.replace("{command}", command));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let served = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Into::into)
        .and_then(|runtime| runtime.block_on(serve()));
    match served {
        Ok(()) => ExitCode::SUCCESS,
        // Stdout carries the protocol: errors go to stderr, which clients log.
        Err(e) => {
            eprintln!("{command}: {e}");
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
