//! The MCP server: the [`Agent`] tools over JSON-RPC on stdio, with `rmcp`.

use std::sync::Arc;

use crate::{Agent, AgentError, QueryArgs};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, Peer, RoleServer, ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

/// What agents read first: how to use the tools.
const INSTRUCTIONS: &str = "PlatypusDB gives access to the database connections the user saved \
in the PlatypusDB app. Start with list_connections, then explore with list_schemas, \
list_objects, describe_table, get_ddl and search_objects, and query with run_query: one \
statement per call, with a row limit, a byte limit and a timeout. Each connection has an \
agent access set by the user: read_only connections run only statements that change nothing, \
inside a read-only transaction; read_write connections also accept changes, which commit at \
once. Production connections are always read-only. Passwords never reach you.";

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ConnectionArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SchemaArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
    /// The schema (a database in MySQL, `main` in SQLite and DuckDB).
    schema: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TableArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
    /// The table's schema.
    schema: String,
    /// The table or view.
    table: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DdlArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
    /// The object's schema.
    schema: String,
    /// The object's name, as `list_objects` shows it.
    name: String,
    /// The kind from `list_objects` (`table`, `view`, `function`, ...); looked up when
    /// left out.
    kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
    /// Text the object's name contains, ignoring case.
    text: String,
}

#[derive(Clone)]
pub struct PlatypusServer {
    agent: Arc<Agent>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for PlatypusServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatypusServer")
            .field("agent", &self.agent)
            .finish_non_exhaustive()
    }
}

/// A tool's answer: its text, or the error as a tool error the agent reads.
#[expect(
    clippy::unnecessary_wraps,
    reason = "tools answer every failure in the result, not as a protocol error"
)]
fn reply(result: Result<String, AgentError>) -> Result<CallToolResult, ErrorData> {
    Ok(match result {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
    })
}

fn json_reply(
    result: Result<impl serde::Serialize, AgentError>,
) -> Result<CallToolResult, ErrorData> {
    reply(result.map(|value| serde_json::to_string(&value).unwrap_or_default()))
}

#[tool_router]
impl PlatypusServer {
    pub fn new(agent: Arc<Agent>) -> Self {
        Self {
            agent,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Lists the database connections saved in PlatypusDB that agents may use: \
        name, id, driver, type (dev, test, prod), database and agent_access (read_only or \
        read_write). Never shows hosts, users or passwords."
    )]
    async fn list_connections(&self) -> Result<CallToolResult, ErrorData> {
        json_reply(self.agent.list_connections())
    }

    #[tool(
        description = "Lists the schemas of a connection (databases in MySQL and MariaDB, \
        `main` and attached databases in SQLite), with `current` for the one unqualified \
        names resolve to and `system` for catalog schemas."
    )]
    async fn list_schemas(
        &self,
        Parameters(args): Parameters<ConnectionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        json_reply(self.agent.list_schemas(&args.connection).await)
    }

    #[tool(
        description = "Lists the tables, views, sequences, functions, procedures, triggers \
        and types of a schema, with their kind, comment and estimated rows."
    )]
    async fn list_objects(
        &self,
        Parameters(args): Parameters<SchemaArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        json_reply(
            self.agent
                .list_objects(&args.connection, &args.schema)
                .await,
        )
    }

    #[tool(
        description = "Describes a table or view: columns (type, nullable, default, primary \
        key, comment, enum values), indexes, foreign keys, other constraints and the foreign \
        keys of other tables that point at it."
    )]
    async fn describe_table(
        &self,
        Parameters(args): Parameters<TableArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        json_reply(
            self.agent
                .describe_table(&args.connection, &args.schema, &args.table)
                .await,
        )
    }

    #[tool(
        description = "Returns the CREATE statement of a table, view, function, trigger or other object."
    )]
    async fn get_ddl(
        &self,
        Parameters(args): Parameters<DdlArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        reply(
            self.agent
                .object_ddl(
                    &args.connection,
                    &args.schema,
                    &args.name,
                    args.kind.as_deref(),
                )
                .await,
        )
    }

    #[tool(
        description = "Finds objects of a connection whose name contains a text, ignoring \
        case; where the database supports it, also objects with such a column or in whose \
        source the text appears. At most 200 matches."
    )]
    async fn search_objects(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        json_reply(
            self.agent
                .search_objects(&args.connection, &args.text)
                .await,
        )
    }

    #[tool(
        description = "Runs one SQL statement on a connection and returns its rows as an \
        aligned text table (or JSON with format=json, where integers beyond 2^53 are strings \
        so that no JSON parser rounds them), saying when rows or long values were left out. Defaults: 100 rows, 32768 bytes, 30 s timeout; max_rows, max_bytes and \
        timeout_seconds raise them up to 10000 rows, 1048576 bytes and 300 s. On read_only \
        connections only statements that change nothing run, in a read-only transaction; on \
        read_write connections changes commit at once, and UPDATE or DELETE without WHERE is \
        refused unless the user's safety settings allow it. Use the database's own SQL dialect."
    )]
    async fn run_query(
        &self,
        Parameters(args): Parameters<QueryArgs>,
        peer: Peer<RoleServer>,
        cancelled: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        // The history trims and caps the name, and calls a client without one `agent`.
        let origin = peer
            .peer_info()
            .map(|info| info.client_info.name.clone())
            .unwrap_or_default();
        let format = args.format.unwrap_or_default();
        let result = self
            .agent
            // `notifications/cancelled` for this request cancels the statement.
            .run_query_until(&args, &origin, cancelled.cancelled_owned())
            .await
            .map(|r| r.render(format));
        reply(result)
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "rmcp's tool_handler writes async methods that need no await"
)]
#[tool_handler(router = self.tool_router)]
impl ServerHandler for PlatypusServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("platypus-mcp", platypus_core::VERSION))
            .with_instructions(INSTRUCTIONS)
    }
}
