//! The MCP server: the [`Agent`] tools over JSON-RPC on stdio, with `rmcp`.

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use crate::{Agent, AgentError, ExplainArgs, QueryArgs, SampleArgs, origin_name};
use platypus_store::{HistoryStatus, NewAgentCall};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, Peer, RoleServer, ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// What agents read first: how to use the tools.
const INSTRUCTIONS: &str = "PlatypusDB gives access to the database connections the user saved \
in the PlatypusDB app. Start with list_connections, then explore with list_schemas, \
list_objects, describe_table, get_ddl and search_objects, and query with run_query: one \
statement per call, with a row limit, a byte limit and a timeout. Each connection has an \
agent access set by the user: read_only connections run only statements that change nothing, \
inside a read-only transaction; read_write connections also accept changes, which commit at \
once. Production connections are always read-only. Passwords never reach you.";

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct ConnectionArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct ObjectsArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
    /// The schema (a database in MySQL, `main` in SQLite and DuckDB).
    schema: String,
    /// Objects to skip: the `next_offset` of the previous page.
    offset: Option<u64>,
    /// Objects returned at most: 500 by default, up to 2000.
    limit: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct TableArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
    /// The table's schema.
    schema: String,
    /// The table or view.
    table: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
    /// Text the object's name contains, ignoring case.
    text: String,
}

#[derive(Clone)]
pub struct PlatypusServer {
    agent: Arc<Agent>,
    /// This run of the server, which groups its calls in the log.
    session: String,
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

fn json_text(result: Result<impl Serialize, AgentError>) -> Result<String, AgentError> {
    result.map(|value| serde_json::to_string(&value).unwrap_or_default())
}

/// A tool call being answered, as the call log records it.
struct Call {
    tool: &'static str,
    agent: String,
    connection: Option<String>,
    /// The arguments given, as JSON text.
    arguments: String,
    started_at: SystemTime,
    start: Instant,
}

impl Call {
    fn new(tool: &'static str, peer: &Peer<RoleServer>, args: &impl Serialize) -> Self {
        let mut arguments = serde_json::to_value(args).unwrap_or_default();
        // Arguments left out serialize as null: the log keeps those given.
        if let Value::Object(map) = &mut arguments {
            map.retain(|_, v| !v.is_null());
        }
        Self {
            tool,
            agent: origin_name(&client_name(peer)),
            connection: arguments
                .get("connection")
                .and_then(Value::as_str)
                .map(str::to_owned),
            arguments: arguments.to_string(),
            started_at: SystemTime::now(),
            start: Instant::now(),
        }
    }
}

#[tool_router]
impl PlatypusServer {
    pub fn new(agent: Arc<Agent>) -> Self {
        Self {
            agent,
            session: uuid::Uuid::new_v4().to_string(),
            tool_router: Self::tool_router(),
        }
    }

    /// The answer to `call`, recorded in the agents' call log.
    fn answer(
        &self,
        call: Call,
        result: Result<String, AgentError>,
    ) -> Result<CallToolResult, ErrorData> {
        let (status, error) = match &result {
            Ok(_) => (HistoryStatus::Ok, None),
            Err(e @ (AgentError::Timeout(_) | AgentError::Cancelled)) => {
                (HistoryStatus::Cancelled, Some(e.to_string()))
            }
            Err(e) => (HistoryStatus::Error, Some(e.to_string())),
        };
        self.agent.record_call(&NewAgentCall {
            session: self.session.clone(),
            agent: call.agent,
            tool: call.tool.to_owned(),
            connection: call.connection,
            arguments: call.arguments,
            started_at: call.started_at,
            duration: call.start.elapsed(),
            status,
            error,
        });
        reply(result)
    }

    #[tool(
        description = "Lists the database connections saved in PlatypusDB that agents may use: \
        name, id, driver, type (dev, test, prod), database and agent_access (read_only or \
        read_write). Never shows hosts, users or passwords."
    )]
    async fn list_connections(&self, peer: Peer<RoleServer>) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("list_connections", &peer, &serde_json::json!({}));
        self.answer(call, json_text(self.agent.list_connections()))
    }

    #[tool(
        description = "Lists the schemas of a connection (databases in MySQL and MariaDB, \
        `main` and attached databases in SQLite), with `current` for the one unqualified \
        names resolve to and `system` for catalog schemas."
    )]
    async fn list_schemas(
        &self,
        Parameters(args): Parameters<ConnectionArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("list_schemas", &peer, &args);
        self.answer(
            call,
            json_text(self.agent.list_schemas(&args.connection).await),
        )
    }

    #[tool(
        description = "Lists the tables, views, sequences, functions, procedures, triggers \
        and types of a schema, with their kind, comment and estimated rows: a list of up to \
        500 objects (limit raises it to 2000). A schema with more returns \
        {objects, truncated: true, total, offset, next_offset}: call again with \
        offset=next_offset for the next page, or use search_objects to find names."
    )]
    async fn list_objects(
        &self,
        Parameters(args): Parameters<ObjectsArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("list_objects", &peer, &args);
        let result = self
            .agent
            .list_objects(&args.connection, &args.schema, args.offset, args.limit)
            .await;
        self.answer(call, json_text(result))
    }

    #[tool(
        description = "Describes a table or view: columns (type, nullable, default, primary \
        key, comment, enum values), indexes, foreign keys, other constraints and the foreign \
        keys of other tables that point at it."
    )]
    async fn describe_table(
        &self,
        Parameters(args): Parameters<TableArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("describe_table", &peer, &args);
        let result = self
            .agent
            .describe_table(&args.connection, &args.schema, &args.table)
            .await;
        self.answer(call, json_text(result))
    }

    #[tool(
        description = "Returns the CREATE statement of a table, view, function, trigger or other object."
    )]
    async fn get_ddl(
        &self,
        Parameters(args): Parameters<DdlArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("get_ddl", &peer, &args);
        let result = self
            .agent
            .object_ddl(
                &args.connection,
                &args.schema,
                &args.name,
                args.kind.as_deref(),
            )
            .await;
        self.answer(call, result)
    }

    #[tool(
        description = "Finds objects of a connection whose name contains a text, ignoring \
        case; where the database supports it, also objects with such a column or in whose \
        source the text appears. At most 200 matches."
    )]
    async fn search_objects(
        &self,
        Parameters(args): Parameters<SearchArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("search_objects", &peer, &args);
        let result = self
            .agent
            .search_objects(&args.connection, &args.text)
            .await;
        self.answer(call, json_text(result))
    }

    #[tool(
        description = "Runs one SQL statement on a connection and returns its rows as an \
        aligned text table (or JSON with format=json, where integers beyond 2^53 are strings \
        so that no JSON parser rounds them), saying when rows or long values were left out. In \
        the text table SQL NULL is NULL, and a text that reads NULL shows quoted, 'NULL'. Defaults: 100 rows, 32768 bytes, 30 s timeout; max_rows, max_bytes and \
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
        let call = Call::new("run_query", &peer, &args);
        let format = args.format.unwrap_or_default();
        let result = self
            .agent
            // `notifications/cancelled` for this request cancels the statement.
            .run_query_until(&args, &call.agent, cancelled.cancelled_owned())
            .await
            .map(|r| r.render(format));
        self.answer(call, result)
    }

    #[tool(
        description = "Returns the first rows of a table, view or MongoDB collection (default         20 rows), as run_query returns them: an aligned text table, or JSON with format=json.         max_rows, max_bytes and timeout_seconds work as in run_query. The rows come in the         order the database reads them: no order is implied."
    )]
    async fn sample_rows(
        &self,
        Parameters(args): Parameters<SampleArgs>,
        peer: Peer<RoleServer>,
        cancelled: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("sample_rows", &peer, &args);
        let format = args.format.unwrap_or_default();
        let result = self
            .agent
            .sample_rows_until(&args, &call.agent, cancelled.cancelled_owned())
            .await
            .map(|r| r.render(format));
        self.answer(call, result)
    }

    #[tool(
        description = "Returns the plan of one SQL statement: its steps as an indented tree         with cost, estimated rows and, with analyze=true, actual rows and times (or JSON with         format=json), then index suggestions for filtered full scans of large tables. Without         analyze the statement does not run. With analyze it runs and what it changes is         rolled back; on read_only connections only statements that change nothing are         explained, and analyze needs PostgreSQL, MySQL, MariaDB or SQLite. max_bytes         (default 32768) and timeout_seconds (default 30) work as in run_query."
    )]
    async fn explain(
        &self,
        Parameters(args): Parameters<ExplainArgs>,
        peer: Peer<RoleServer>,
        cancelled: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("explain", &peer, &args);
        let format = args.format.unwrap_or_default();
        let result = self
            .agent
            .explain_until(&args, cancelled.cancelled_owned())
            .await
            .map(|r| r.render(format));
        self.answer(call, result)
    }
}

/// The client's name as it introduced itself; [`origin_name`] trims and caps it, and calls
/// a client without one `agent`.
fn client_name(peer: &Peer<RoleServer>) -> String {
    peer.peer_info()
        .map(|info| info.client_info.name.clone())
        .unwrap_or_default()
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
