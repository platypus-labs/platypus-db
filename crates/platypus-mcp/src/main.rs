//! `platypus-mcp`: PlatypusDB's MCP server as a binary of its own, for development and
//! builds from source. The installed app serves the same with `platypus-db --mcp`.
//!
//! ```text
//! claude mcp add platypus -- /path/to/platypus-mcp
//! ```
//!
//! It uses the connections of the app's profile, or of `PLATYPUS_DATA_DIR` when set.

use std::process::ExitCode;

fn main() -> ExitCode {
    platypus_mcp::stdio::main("platypus-mcp", std::env::args().skip(1))
}
