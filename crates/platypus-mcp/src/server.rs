//! The MCP server: the [`Agent`] tools over JSON-RPC, with `rmcp`, on stdio or (with the
//! `http` feature, in the app) on the bridge's local port.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::agent::{MAX_OBJECT_PAGE, locked};
use crate::cursor::SessionCursors;
use crate::er::DiagramFormat;
use crate::link::CLOSED;
use crate::prompts::{self, PromptError};
use crate::resource::{self, Target};
use crate::watch::Subscriptions;
use crate::{AcrossArgs, EstimateArgs, ProposeEditsArgs, ServerViewArgs};
use crate::{
    Agent, AgentError, CompareArgs, ContextArgs, ExplainArgs, HistoryArgs, ListScriptsArgs,
    PageArgs, ProfileArgs, ProposeArgs, QueryArgs, ReadScriptArgs, SampleArgs, origin_name,
};
use platypus_store::{HistoryStatus, NewAgentCall};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, GetPromptRequestParams, GetPromptResponse, GetPromptResult,
    Implementation, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult,
    PaginatedRequestParams, PromptMessage, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, ResourceTemplate, Role, ServerCapabilities,
    ServerConfig, SubscribeRequestParams, UnsubscribeRequestParams,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, Peer, RoleServer, ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// What agents read first: how to use the tools.
const INSTRUCTIONS: &str = "PlatypusDB gives access to the database connections the user saved \
in the PlatypusDB app. Start with list_connections, then explore with list_schemas, \
list_objects, describe_table, get_ddl, get_er_diagram, compare_schemas and search_objects, look at a table's first rows with \
sample_rows, profile its columns with profile_table (sensitive columns give counts only), query with run_query (next_page reads the rest of a cut result; query_across_databases joins tables of several connections) and read a statement's plan with explain, and ask estimate_change what a change would show the \
user before it runs (nothing runs); server_activity and top_queries read what the server does: one statement per \
call, with a row limit, a byte limit and a timeout. To hand the user SQL to review and run by \
hand (a change the access does not allow, a long script), use propose_sql; to hand the user rows \
to change in a table, use propose_edits: they open in its data viewer as pending edits to save or \
discard. Each connection has an \
agent access set by the user: read_only connections run only statements that change nothing, \
inside a read-only transaction; read_write connections also accept changes, which commit at \
once. Production connections are always read-only. Passwords never reach you. While the \
PlatypusDB app is open, list_open_sessions shows what the user has open there, and changes may \
wait for the user to allow them in the app; when they say \"this query\", \"what I selected\" or \
\"the result I see\", get_user_context asks them to share it. list_scripts and read_script \
show the scripts the user saved, search_history the statements run before, to learn how the user \
queries the data. server_activity (sessions, running statements, locks) and top_queries (the \
statements that cost the server most time) show what the server is doing, read-only, with the \
statements' values masked. Connections, schemas and tables are also MCP \
resources (platypus://<connection>/<schema>/<table>), for clients that attach them as context.";

const JSON: &str = "application/json";

/// How often the server tells the open app it is still connected; the app forgets an agent
/// after three times as long.
pub const HELLO_EVERY: Duration = Duration::from_secs(30);

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
struct ErDiagramArgs {
    /// The connection's name, as `list_connections` shows it.
    connection: String,
    /// The schema (a database in MySQL, `main` in SQLite and DuckDB).
    schema: String,
    /// Tables to draw, as `list_objects` shows them; left out, the whole schema.
    tables: Option<Vec<String>>,
    /// With `tables`: how many foreign-key hops from them to follow, in both directions
    /// (default 1, at most 3).
    depth: Option<u64>,
    /// `mermaid` (the default: text, works without the app) or `svg` (an image drawn by the
    /// open PlatypusDB app, as its diagram export; an error when the app is closed).
    format: Option<String>,
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
    /// When the open app was last told this server is connected.
    greeted: Arc<Mutex<Option<Instant>>>,
    /// The resources the client subscribed to; nothing runs while there are none.
    subscriptions: Arc<Subscriptions>,
    /// Closes the cursors of this session when its last clone goes.
    _cursors: Arc<SessionCursors>,
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
        let session = uuid::Uuid::new_v4().to_string();
        Self {
            _cursors: Arc::new(SessionCursors::new(Arc::clone(&agent), session.clone())),
            agent,
            session,
            greeted: Arc::default(),
            subscriptions: Arc::default(),
            tool_router: Self::tool_router(),
        }
    }

    /// Tells the open app that the agent `agent` is connected, unless it was told less than
    /// [`HELLO_EVERY`] ago. Nothing waits for it: an app that is closed is not told.
    pub fn greet(&self, agent: &str) {
        let Some(link) = self.agent.link() else {
            return;
        };
        {
            let mut greeted = locked(&self.greeted);
            if greeted.is_some_and(|at| at.elapsed() < HELLO_EVERY) {
                return;
            }
            *greeted = Some(Instant::now());
        }
        let (link, session, agent) = (Arc::clone(link), self.session.clone(), agent.to_owned());
        let greeted = Arc::clone(&self.greeted);
        tokio::spawn(async move {
            // An app that opens later is told at the next call.
            if link.hello(&session, &agent).await.is_err() {
                *locked(&greeted) = None;
            }
        });
    }

    /// Greets the open app every [`HELLO_EVERY`] for the client of `peer`, so that it shows
    /// the agent connected while the server runs.
    #[must_use]
    pub fn keep_greeting(&self, peer: Peer<RoleServer>) -> tokio::task::JoinHandle<()> {
        let server = self.clone();
        tokio::spawn(async move {
            loop {
                server.greet(&origin_name(&client_name(&peer)));
                tokio::time::sleep(HELLO_EVERY).await;
            }
        })
    }

    /// Records `call` in the agents' call log, with the outcome of `result`.
    /// The write goes to a blocking thread: the app may hold the file for a moment.
    async fn record<T>(&self, call: Call, result: &Result<T, AgentError>) {
        // A timeout is the call failing; only the client's own cancellation is not.
        let (status, error) = match result {
            Ok(_) => (HistoryStatus::Ok, None),
            Err(e @ AgentError::Cancelled) => (HistoryStatus::Cancelled, Some(e.to_string())),
            Err(e) => (HistoryStatus::Error, Some(e.to_string())),
        };
        let entry = NewAgentCall {
            session: self.session.clone(),
            agent: call.agent,
            tool: call.tool.to_owned(),
            connection: call.connection,
            arguments: call.arguments,
            started_at: call.started_at,
            duration: call.start.elapsed(),
            status,
            error,
        };
        self.greet(&entry.agent);
        let agent = Arc::clone(&self.agent);
        let _ = tokio::task::spawn_blocking(move || agent.record_call(&entry)).await;
    }

    /// The answer to `call`, recorded in the agents' call log.
    async fn answer(
        &self,
        call: Call,
        result: Result<String, AgentError>,
    ) -> Result<CallToolResult, ErrorData> {
        self.record(call, &result).await;
        reply(result)
    }

    /// The JSON of the resource `target` points at.
    async fn resource_json(&self, target: &Target) -> Result<String, AgentError> {
        match target {
            Target::Connection(c) => json_text(self.agent.list_schemas(c).await),
            Target::Schema(c, s) => json_text(
                self.agent
                    .list_objects(c, s, None, u64::try_from(MAX_OBJECT_PAGE).ok())
                    .await,
            ),
            Target::Table(c, s, t) => json_text(self.agent.describe_table(c, s, t).await),
        }
    }

    #[tool(
        description = "Lists the database connections saved in PlatypusDB that agents may use: \
        name, id, driver, type (dev, test, prod), database and agent_access (read_only or \
        read_write). Never shows hosts, users or passwords."
    )]
    async fn list_connections(&self, peer: Peer<RoleServer>) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("list_connections", &peer, &serde_json::json!({}));
        self.answer(call, json_text(self.agent.list_connections()))
            .await
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
        let result = self.agent.list_schemas(&args.connection).await;
        self.answer(call, json_text(result)).await
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
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "Describes a table or view: columns (type, nullable, default, primary \
        key, comment, enum values), indexes, foreign keys, other constraints and the foreign \
        keys of other tables that point at it. Also the notes, tags and sensitive marks the user \
        set in PlatypusDB (note, tags, sensitive: true or false, never values), when there are any."
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
        self.answer(call, json_text(result)).await
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
        self.answer(call, result).await
    }

    #[tool(
        description = "Returns a Mermaid erDiagram of a schema's tables (columns, primary keys) \
        and the foreign keys between them, as text. Without `tables` the whole schema; with \
        `tables`, those and the tables within `depth` foreign-key hops of them (default 1, \
        at most 3). A diagram too long for one answer is cut with a note: pass `tables` to choose. \
        With `format: \"svg\"` the answer is the SVG image PlatypusDB draws for its own export \
        (it needs the app open, asks the user nothing, and is refused rather than cut when it is \
        over the byte limit); the default `mermaid` works without the app."
    )]
    async fn get_er_diagram(
        &self,
        Parameters(args): Parameters<ErDiagramArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("get_er_diagram", &peer, &args);
        let tables = args.tables.as_deref().unwrap_or_default();
        let result = match DiagramFormat::parse(args.format.as_deref()) {
            Err(e) => Err(e),
            Ok(DiagramFormat::Mermaid) => {
                self.agent
                    .er_diagram(&args.connection, &args.schema, tables, args.depth)
                    .await
            }
            Ok(DiagramFormat::Svg) => {
                self.agent
                    .er_diagram_svg(
                        &args.connection,
                        &args.schema,
                        tables,
                        args.depth,
                        (&call.agent, &self.session),
                    )
                    .await
            }
        };
        self.answer(call, result).await
    }

    #[tool(
        description = "Compares two schemas, of one connection or of two connections of the \
        same kind of database, as the PlatypusDB schema compare does: the differences (tables, \
        columns, indexes, foreign keys and constraints added, dropped or changed) and the \
        migration script that makes `other_schema` like `schema`, as text. Nothing runs: the \
        script reaches the user only through propose_sql. A script too long for max_bytes \
        (default 32768) is left out whole and the answer says so: it is never cut."
    )]
    async fn compare_schemas(
        &self,
        Parameters(args): Parameters<CompareArgs>,
        peer: Peer<RoleServer>,
        cancelled: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("compare_schemas", &peer, &args);
        let result = self
            .agent
            .compare_schemas_until(&args, cancelled.cancelled_owned())
            .await;
        self.answer(call, result).await
    }

    #[tool(
        description = "Profiles the columns of a table or view from a sample of its rows (the \
        first sample_rows, default 10000, up to 100000; no order is implied): per column NULLs, \
        distinct values, empty texts, minimum and maximum, the most frequent values, patterns \
        (emails, dates, numbers stored as text...) and, for numbers, their spread. Sensitive \
        columns (as in results) give counts only: no value of any kind. Read-only, within the \
        connection's limits; max_bytes and timeout_seconds work as in run_query."
    )]
    async fn profile_table(
        &self,
        Parameters(args): Parameters<ProfileArgs>,
        peer: Peer<RoleServer>,
        cancelled: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("profile_table", &peer, &args);
        let result = self
            .agent
            .profile_table_until(&args, cancelled.cancelled_owned())
            .await;
        self.answer(call, result).await
    }

    #[tool(
        description = "Finds objects of a connection whose name contains a text, ignoring \
        case; where the database supports it, also objects with such a column or in whose \
        source the text appears; also objects whose comment or the user's notes contain it \
        (matched comment or note), and #tag finds the objects the user tagged. At most 200 matches."
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
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "Runs one SQL statement on a connection and returns its rows as an \
        aligned text table (or JSON with format=json, where integers beyond 2^53 are strings \
        so that no JSON parser rounds them), saying when rows or long values were left out. In \
        the text table SQL NULL is NULL, and a text that reads NULL shows quoted, 'NULL'. Defaults: 100 rows, 32768 bytes, 30 s timeout; max_rows, max_bytes and \
        timeout_seconds raise them up to 10000 rows, 1048576 bytes and 300 s. On read_only \
        connections only statements that change nothing run, in a read-only transaction; on \
        read_write connections changes commit at once, and UPDATE or DELETE without WHERE is \
        refused unless the user's safety settings allow it. Use the database's own SQL dialect. A result cut by max_rows or max_bytes returns a `cursor` (the `cursor` field in JSON) for next_page to read the rest."
    )]
    async fn run_query(
        &self,
        Parameters(args): Parameters<QueryArgs>,
        peer: Peer<RoleServer>,
        cancelled: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("run_query", &peer, &args);
        let result = self
            .agent
            // `notifications/cancelled` for this request cancels the statement.
            .run_query_page(
                &args,
                (&call.agent, &self.session),
                cancelled.cancelled_owned(),
            )
            .await;
        self.answer(call, result).await
    }

    #[tool(
        description = "Reads the next rows of a result that run_query cut at max_rows or \
        max_bytes, from the cursor it returned, instead of running the statement again: the \
        pages come from the rows read with the first one (up to 10000 more rows or 8 MB; \
        the answer says when the result has more than that), so they do not change if the \
        data does. Same format, max_rows (default 100) and max_bytes as run_query, with the \
        connection's limits and calls per minute; sensitive columns stay masked as in the first \
        page. A cursor is kept 5 minutes after its last use and belongs to this session; an \
        unknown, expired or another session's cursor is refused."
    )]
    async fn next_page(
        &self,
        Parameters(args): Parameters<PageArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("next_page", &peer, &args);
        let result = self.agent.next_page(&args, &self.session);
        self.answer(call, result).await
    }

    #[tool(
        description = "Runs one read-only DuckDB statement over tables of several connections: \
        each source (connection, schema, table, optional name) is copied, up to source_rows rows \
        (default 10000, up to 100000) and within that connection's limits, into a DuckDB of \
        this call, where the statement joins them by name. Every connection must be open to \
        agents; the statement runs only on the copies, cannot write to a source and reads no \
        file nor URL. Values of sensitive columns are masked before they are copied (unless the \
        user shows them for that connection): all values of such a column are equal, so do not \
        join on one. A source copied in part is said so. Use DuckDB's SQL. Same result format \
        and limits as run_query."
    )]
    async fn query_across_databases(
        &self,
        Parameters(args): Parameters<AcrossArgs>,
        peer: Peer<RoleServer>,
        cancelled: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("query_across_databases", &peer, &args);
        let result = self
            .agent
            .query_across_until(&args, cancelled.cancelled_owned())
            .await;
        self.answer(call, result).await
    }

    #[tool(
        description = "Returns the first rows of a table, view, MongoDB collection or search index (default \
        20 rows), as run_query returns them: an aligned text table, or JSON with format=json. \
        max_rows, max_bytes and timeout_seconds work as in run_query. The rows come in the \
        order the database reads them: no order is implied."
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
        self.answer(call, result).await
    }

    #[tool(
        description = "Returns the plan of one SQL statement: its steps as an indented tree \
        with cost, estimated rows and, with analyze=true, actual rows and times (or JSON with \
        format=json), then index suggestions for filtered full scans of large tables. Without \
        analyze the statement does not run. With analyze it runs and what it changes is \
        rolled back; on read_only connections only statements that change nothing are \
        explained, and analyze needs PostgreSQL, MySQL, MariaDB or SQLite. max_bytes \
        (default 32768) and timeout_seconds (default 30) work as in run_query."
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
        self.answer(call, result).await
    }

    #[tool(
        description = "Dry run of a change: returns, without running anything, what the \
        confirmation dialog would show the user for one SQL statement: the rows the database \
        expects to change (estimated_rows) and its plan, the Safety checks that would ask or \
        refuse (safety_checks), and whether the user is asked in the app (asks_the_user) and \
        the change would run (would_run). A statement the connection's access refuses answers \
        outcome=refused with the reason; one that changes nothing answers \
        outcome=not_a_change. A database without plans answers with no estimate and says so. \
        Use it to correct a statement before run_query asks the user."
    )]
    async fn estimate_change(
        &self,
        Parameters(args): Parameters<EstimateArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("estimate_change", &peer, &args);
        let result = self.agent.estimate_change(&args).await;
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "What the database server is doing now: its sessions (user, database, \
        client, state, the statement running and for how long, what it waits for and the \
        sessions blocking it) and the locks held or waited for, as in the PlatypusDB admin \
        tab; sessions that wait or run first, 50 by default (limit, up to 200). The \
        statements are other people's: credentials and the values of sensitive columns are \
        masked, and long ones cut at 500 characters. PostgreSQL, MySQL, MariaDB and SQL \
        Server; read-only: nothing is cancelled or ended. A missing privilege is said so."
    )]
    async fn server_activity(
        &self,
        Parameters(args): Parameters<ServerViewArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("server_activity", &peer, &args);
        let result = self.agent.server_activity(&args).await;
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "The statements that took the server most time, the costliest first, from \
        its own statistics (pg_stat_statements, performance_schema, Query Store): the \
        statement with its values replaced as the server writes them, calls, total, mean and \
        max seconds, rows, user and database. 20 by default (limit, up to 100). Credentials \
        and the values of sensitive columns are masked, long statements cut at 500 characters. \
        PostgreSQL, MySQL, MariaDB and SQL Server; read-only. Says so when the statistics are \
        not enabled (pg_stat_statements not installed) or the user may not read them."
    )]
    async fn top_queries(
        &self,
        Parameters(args): Parameters<ServerViewArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("top_queries", &peer, &args);
        let result = self.agent.top_queries(&args).await;
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "Lists the SQL scripts the user saved in PlatypusDB: path (relative to \
        the project, with `/`), whether it is a folder, and the name of the connection a script \
        opens on, if it is linked to one. project is the project's name as the app shows it; \
        left out, the scripts of All connections. Scripts linked to a connection agents may not \
        use are not listed. Read-only: use read_script for a script's text."
    )]
    async fn list_scripts(
        &self,
        Parameters(args): Parameters<ListScriptsArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("list_scripts", &peer, &args);
        let result = self.agent.list_scripts(&args);
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "Returns the text of a script the user saved in PlatypusDB, by the path \
        list_scripts shows, with the connection it is linked to. At most 256 KiB: a longer \
        script is cut, and the answer says so (truncated). A script linked to a connection \
        agents may not use is refused. Read-only: nothing is run or changed."
    )]
    async fn read_script(
        &self,
        Parameters(args): Parameters<ReadScriptArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("read_script", &peer, &args);
        let result = self.agent.read_script(&args);
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "Searches the history of statements run in PlatypusDB, newest first, on \
        the connections agents may use: connection, sql, started_at (UTC, RFC 3339), \
        duration_ms, rows, status (ok, error or cancelled), error and origin (`app` for what \
        the user ran, else the agent's name). connection limits it to one connection, text to \
        statements containing it (ignoring case), origin to `app` or `agent`. 50 entries by \
        default, up to 500 with limit. Use it to see how the user queries the data."
    )]
    async fn search_history(
        &self,
        Parameters(args): Parameters<HistoryArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("search_history", &peer, &args);
        let result = self.agent.search_history(&args);
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "Lists the sessions open in the PlatypusDB app, when it is open: their \
        connection, the database when it is not the connection's own, purpose (`main` for the \
        session the connection's editor tabs and navigator share, `side` for one a dialog \
        opened), whether it runs in manual-commit mode and whether a transaction is open in it. \
        Your statements run on sessions of your own, never on these: use it to know what the \
        user is working on."
    )]
    async fn list_open_sessions(
        &self,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("list_open_sessions", &peer, &serde_json::json!({}));
        let result = match self.agent.link() {
            Some(link) => link
                .sessions()
                .await
                .map_err(|e| AgentError::Store(format!("cannot list the app's sessions: {e}"))),
            None => Err(AgentError::Store(CLOSED.to_owned())),
        };
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "Asks the user to share what they are looking at in the PlatypusDB app: \
        the text of the active editor tab (editor), the part they selected (selection) and the \
        result their grid shows (result: columns and rows). Use it when the user refers to \
        something on their screen: \"this query\", \"what I selected\", \"the result I see\". \
        Nothing is shared unless the user allows it in the app, each time or for your whole \
        session; they can refuse, with a reason you get. parts limits what you ask for (all \
        three by default). Answers connection, tab (its title) and the parts shared; the editor \
        and selection are cut at 64 KiB, the result at 100 rows and 32 KiB (truncated says so). \
        A tab on a connection that is not open to agents is not shared. Needs the app open; \
        read-only: nothing runs."
    )]
    async fn get_user_context(
        &self,
        Parameters(args): Parameters<ContextArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("get_user_context", &peer, &args);
        let result = self
            .agent
            .user_context(&args, (&call.agent, &self.session))
            .await;
        self.answer(call, json_text(result)).await
    }

    #[tool(
        description = "Proposes SQL to the user: PlatypusDB opens it in a new editor tab on the \
        connection (now if the app is open, else when it starts), where the user reviews, edits \
        and runs it by hand. Nothing runs here, whatever the connection's agent access: use it \
        for changes the user should check, or that read_only access does not allow. title names \
        the tab."
    )]
    async fn propose_sql(
        &self,
        Parameters(args): Parameters<ProposeArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("propose_sql", &peer, &args);
        let (agent, name) = (Arc::clone(&self.agent), call.agent.clone());
        let result = tokio::task::spawn_blocking(move || agent.propose_sql(&args, &name))
            .await
            .unwrap_or_else(|e| Err(AgentError::Store(e.to_string())));
        self.answer(call, result).await
    }

    #[tool(
        description = "Proposes rows to change in a table: PlatypusDB opens the table's data \
        viewer with them as pending edits (new rows, changed cells, rows marked for deletion) \
        that the user reviews, saves or discards (now if the app is open, else when it starts). \
        Nothing is written here, whatever the connection's agent access. inserts: objects of \
        column to value. updates: {key, set}, key naming a row by exactly the columns of a \
        primary or unique key. deletes: objects naming rows by such a key. A value is a string, \
        number, boolean or null (no arrays or objects); at most 500 rows and 1 MiB in all. The \
        table needs a primary or unique key; MongoDB, Redis, Elasticsearch and Cassandra, \
        read-only connections and those that forbid editing data are refused. title names the tab."
    )]
    async fn propose_edits(
        &self,
        Parameters(args): Parameters<ProposeEditsArgs>,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = Call::new("propose_edits", &peer, &args);
        let result = self.agent.propose_edits(&args, &call.agent).await;
        self.answer(call, result).await
    }
}

/// The client's name as it introduced itself; [`origin_name`] trims and caps it, and calls
/// a client without one `agent`.
fn client_name(peer: &Peer<RoleServer>) -> String {
    peer.peer_info()
        .map(|info| info.client_info.name.clone())
        .unwrap_or_default()
}

/// The server on HTTP, for the app's bridge: each MCP session of a client is a run of the
/// server over `agent`, until `stop` is cancelled.
#[cfg(feature = "http")]
pub fn http_service(
    agent: Arc<Agent>,
    stop: CancellationToken,
) -> rmcp::transport::streamable_http_server::StreamableHttpService<
    PlatypusServer,
    rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
    };
    let mut config = StreamableHttpServerConfig::default();
    config.cancellation_token = stop;
    StreamableHttpService::new(
        move || Ok(PlatypusServer::new(Arc::clone(&agent))),
        Arc::default(),
        config,
    )
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "rmcp's tool_handler writes async methods that need no await"
)]
#[tool_handler(router = self.tool_router)]
impl ServerHandler for PlatypusServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_resources_subscribe()
                .enable_resources_list_changed()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("platypus-mcp", platypus_core::VERSION))
        .with_instructions(INSTRUCTIONS)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        // Connections only: listing schemas would open every database.
        let connections = self
            .agent
            .list_connections()
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        // Two connections may share a name: those get their id in the URI, which `find`
        // accepts as it does a name.
        let shared = |name: &str| connections.iter().filter(|c| c.name == name).count() > 1;
        let resources = connections
            .iter()
            .map(|c| {
                let driver = serde_json::to_value(c.driver)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let key = if shared(&c.name) {
                    c.id.to_string()
                } else {
                    c.name.clone()
                };
                Resource::new(resource::connection_uri(&key), c.name.clone())
                    .with_description(format!(
                        "The schemas of this {driver} connection, as list_schemas returns them."
                    ))
                    .with_mime_type(JSON)
            })
            .collect();
        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new("platypus://{connection}/{schema}", "schema")
                .with_description(format!(
                    "The objects of a schema (tables, views, functions, ...), as list_objects \
                     returns them, up to {MAX_OBJECT_PAGE}."
                ))
                .with_mime_type(JSON),
            ResourceTemplate::new("platypus://{connection}/{schema}/{table}", "table")
                .with_description(
                    "A table or view described: columns, keys, indexes and foreign keys, as \
                     describe_table returns them.",
                )
                .with_mime_type(JSON),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let Some(target) = resource::parse(&request.uri) else {
            return Err(ErrorData::resource_not_found(
                format!(
                    "unknown resource `{}`: use {}",
                    request.uri,
                    resource::FORMS
                ),
                None,
            ));
        };
        let connection = match &target {
            Target::Connection(c) | Target::Schema(c, _) | Target::Table(c, _, _) => c,
        };
        let args = serde_json::json!({ "uri": request.uri, "connection": connection });
        let call = Call::new("read_resource", &context.peer, &args);
        let result = self.resource_json(&target).await;
        self.record(call, &result).await;
        match result {
            Ok(text) => Ok(ReadResourceResponse::Complete(ReadResourceResult::new(
                vec![ResourceContents::text(text, request.uri).with_mime_type(JSON)],
            ))),
            Err(e) => Err(ErrorData::invalid_params(e.to_string(), None)),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let prompts = prompts::list(&self.agent)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(ListPromptsResult::with_all_items(prompts))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        // Arguments are text; a value of another type is given as its JSON.
        let values = request
            .arguments
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, v.as_str().map_or_else(|| v.to_string(), str::to_owned)))
            .collect();
        match prompts::get(&self.agent, &request.name, &values) {
            Ok(text) => Ok(GetPromptResponse::Complete(GetPromptResult::new(vec![
                PromptMessage::new_text(Role::User, text),
            ]))),
            Err(e @ PromptError::Failed(_)) => Err(ErrorData::internal_error(e.to_string(), None)),
            Err(e) => Err(ErrorData::invalid_params(e.to_string(), None)),
        }
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let Some(
            Target::Connection(connection)
            | Target::Schema(connection, _)
            | Target::Table(connection, _, _),
        ) = resource::parse(&request.uri)
        else {
            return Err(ErrorData::invalid_params(
                format!(
                    "unknown resource `{}`: use {}",
                    request.uri,
                    resource::FORMS
                ),
                None,
            ));
        };
        // A connection with access off is not one agents can use, so it cannot be watched.
        self.agent
            .find(&connection)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        self.subscriptions
            .add(&request.uri, &self.agent, &context.peer);
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.subscriptions.remove(&request.uri);
        Ok(())
    }
}
