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

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Serve,
    Version,
    Help,
    /// An argument the server does not take: a mistyped agent configuration.
    Unknown(String),
}

fn request(args: impl Iterator<Item = String>) -> Request {
    let args: Vec<String> = args.collect();
    match args.as_slice() {
        [] => Request::Serve,
        [one] if matches!(one.as_str(), "--version" | "-V") => Request::Version,
        [one] if matches!(one.as_str(), "--help" | "-h") => Request::Help,
        [one] => Request::Unknown(one.clone()),
        [_, extra, ..] => Request::Unknown(extra.clone()),
    }
}

/// Runs the server until the agent closes stdin. `command` names the server in `--help`,
/// `--version` and errors; `args` are the arguments after it, which may ask for those two.
/// Any other argument prints the usage on stderr and fails, rather than serving.
#[must_use]
pub fn main(command: &str, args: impl Iterator<Item = String>) -> ExitCode {
    match request(args) {
        Request::Version => {
            println!("{command} {}", platypus_core::VERSION);
            return ExitCode::SUCCESS;
        }
        Request::Help => {
            println!("{}", HELP.replace("{command}", command));
            return ExitCode::SUCCESS;
        }
        Request::Unknown(arg) => {
            eprintln!(
                "{command}: unknown argument `{arg}`\n\nUsage: {command} [--help | --version]\n\n\
                 Without arguments it serves MCP on stdin and stdout."
            );
            return ExitCode::from(2);
        }
        Request::Serve => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    fn req(args: &[&str]) -> Request {
        request(args.iter().map(|a| (*a).to_owned()))
    }

    #[test]
    fn unknown_arguments_are_refused() {
        assert_eq!(req(&[]), Request::Serve);
        assert_eq!(req(&["--version"]), Request::Version);
        assert_eq!(req(&["-h"]), Request::Help);
        assert_eq!(req(&["--bogus"]), Request::Unknown("--bogus".to_owned()));
        assert_eq!(
            req(&["--version", "--bogus"]),
            Request::Unknown("--bogus".to_owned())
        );
    }
}
