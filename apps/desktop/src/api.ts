import { Channel, invoke } from "@tauri-apps/api/core";
import { tableFromIPC } from "apache-arrow";
import type { Verdict, WriteRequest } from "./agents/writes";

export type DriverKind =
  | "postgres"
  | "mysql"
  | "sqlite"
  | "duckdb"
  | "sqlserver"
  | "clickhouse"
  | "libsql"
  | "mongo"
  | "redis"
  | "elasticsearch"
  | "snowflake"
  | "bigquery"
  | "databricks"
  | "trino"
  | "adbc"
  | "oracle"
  | "db2"
  | "odbc";

/** Speaks mongosh instead of SQL (MongoDB): no SQL completion, formatter, linter or data editor. */
export const isMongo = (driver: DriverKind) => driver === "mongo";

/** Speaks Redis commands instead of SQL, one per line; its key types are virtual tables. */
export const isRedis = (driver: DriverKind) => driver === "redis";

/** Speaks Elasticsearch or OpenSearch requests (a console's `GET /index/_search` and a JSON
 *  body), and SQL among them; its indices are tables and their documents the rows. */
export const isSearch = (driver: DriverKind) => driver === "elasticsearch";

/** Speaks no SQL of the kind the SQL tools read (MongoDB, Redis, Elasticsearch): no SQL
 *  completion, formatter, linter, query builder, ER diagram, table editor, schema tools or SQL
 *  import. */
export const isNoSql = (driver: DriverKind) => isMongo(driver) || isRedis(driver) || isSearch(driver);

/** Runs in-process on a local file (SQLite, DuckDB): no server, host, user or password. */
export const isEmbedded = (driver: DriverKind) => driver === "sqlite" || driver === "duckdb";
export type SslMode = "disable" | "prefer" | "require" | "verify-ca" | "verify-full";

export interface SavedConnection {
  id: string;
  name: string;
  driver: DriverKind;
  host: string;
  port: number | null;
  database: string;
  user: string;
  ssl_mode: SslMode;
  ssl_root_cert: string | null;
  /** Certificate and key presented to servers that authenticate clients by certificate. */
  ssl_client_cert?: string | null;
  ssl_client_key?: string | null;
  save_password: boolean;
  /** A shell command printing the password, run at each connection. */
  password_command?: string | null;
  /** Looks the password up in `.pgpass`. */
  pgpass?: boolean;
  /** A network profile whose SSH tunnel and SSL settings replace the connection's own. */
  network_profile?: string | null;
  /** Default schema, time zone and bootstrap statements, run after every connect. */
  init?: SavedInit;
  read_only: boolean;
  /** What the app does not do with the connection's data; see `platypus_store::Restrictions`. */
  restrictions?: Restrictions;
  color: string | null;
  /** Production asks before changes and starts in manual commit. */
  connection_type?: ConnectionType | null;
  /** What external agents (MCP, the CLI) may do; production and read-only connections
   *  never let them write. */
  agent_access?: AgentAccess;
  /** JSON in a text column: indented in the value panel (the default), or as written. */
  text_json?: "format" | "plain";
  /** How the form enters the address, as it was left: the host fields or one URL. */
  connect_by?: "host" | "url";
  /** The SSH server the connection goes through. */
  ssh?: SavedSsh | null;
  /** Logical keys set by the user for tables without a primary key. */
  virtual_keys?: VirtualKey[];
  /** What a driver needs beyond the common fields, e.g. Snowflake's `warehouse`. */
  options?: Record<string, string>;
}

export type ConnectionType = "dev" | "test" | "prod";

/** A snapshot of the rows an UPDATE or DELETE was about to change. */
export interface SnapshotMeta {
  id: string;
  connection_id: string;
  connection: string;
  database: string | null;
  verb: "UPDATE" | "DELETE";
  table: string;
  statement: string;
  rows: number;
  /** Milliseconds since the epoch. */
  taken_at: number;
}

export type SnapshotOutcome =
  | { kind: "taken"; snapshot: SnapshotMeta }
  | { kind: "unsupported" }
  | { kind: "too_many"; limit: number }
  | { kind: "no_key" };

export interface Restrictions {
  /** Its data goes neither to files nor to other databases, nor to agents. */
  no_export?: boolean;
  /** Its rows change only by SQL: no data editor, imports or generated rows. */
  no_data_editing?: boolean;
}

/** Whether the connection's data stays in it: no exports, dumps or copies elsewhere. */
export const keepsDataIn = (c: SavedConnection) => !!c.restrictions?.no_export;

/** Whether the app's editor and dialogs write no rows to the connection (SQL may). */
export const takesNoRows = (c: SavedConnection) => c.read_only || !!c.restrictions?.no_data_editing;

export type AgentAccess = "off" | "read_only" | "read_write";

/** Where the logs are, and the report of the crash that ended the last run. */
export interface DiagnosticsInfo {
  log_dir: string | null;
  crash: string | null;
}

/** The app's executable, which agents start with `--mcp`, and the commands that register it. */
export interface AgentSetup {
  mcp_path: string;
  /** `claude mcp add …` for Claude Code. */
  claude_command: string;
  /** The `[mcp_servers.platypus]` table of Codex's `config.toml`. */
  codex_config: string;
}

/** The bridge agents reach the open app through (Settings › Agents). */
export interface BridgeStatus {
  enabled: boolean;
  port: number;
  /** The MCP endpoint, while the bridge runs. */
  url: string | null;
  token: string;
  /** Why it does not run, e.g. the port is taken. */
  error: string | null;
}

/** An agent connected to the bridge. */
export interface ConnectedAgent {
  session: string;
  agent: string;
}

/** Why agents cannot open a connection their access lets them use: it asks for its password
 *  at each connection, or its password is behind the master password. */
export type AgentBlocker = "asks_password" | "locked";

/** An agent registration that still starts `platypus-mcp`, the server earlier versions
 *  installed beside the app. */
export interface StaleRegistration {
  agent: "Claude Code" | "Codex";
  name: string;
  command: string;
  /** The project folder of a Claude Code registration local to a project. */
  project: string | null;
}

export interface SavedInit {
  schema: string | null;
  time_zone: string | null;
  /** A script, split into statements when connecting. */
  statements: string;
}

/** Whether saved passwords are in the file encrypted with a master password. */
export interface VaultStatus {
  enabled: boolean;
  /** The master password was entered in this run of the app. */
  unlocked: boolean;
}

/** An SSH tunnel and SSL settings shared by several connections. */
export interface NetworkProfile {
  id: string;
  name: string;
  ssl_mode: SslMode;
  ssl_root_cert: string | null;
  ssl_client_cert: string | null;
  ssl_client_key: string | null;
  ssh: SavedSsh | null;
}

export type SshAuthKind = "password" | "key" | "agent";

/** An SSH server; its password or key passphrase is in the keychain under `secret_id`. */
export interface SavedSsh {
  host: string;
  port: number;
  user: string;
  auth: SshAuthKind;
  key_path: string | null;
  secret_id: string;
  /** A jump host to reach this server through. */
  jump: SavedSsh | null;
}

/** SSH passwords or passphrases typed in a form, by `secret_id`. */
export type SshSecrets = Record<string, PasswordInput>;

export type ImportSource = "dbeaver" | "datagrip" | "pgadmin" | "pg_service";

export interface ImportFile {
  source: ImportSource;
  path: string;
}

/** Columns that identify a table's rows, set by the user instead of a primary key. */
export interface VirtualKey {
  schema: string;
  table: string;
  columns: string[];
}

export type PasswordInput =
  | { action: "keep" }
  | { action: "set"; value: string }
  | { action: "clear" };

export interface ServerInfo {
  product: string;
  version: string;
  encrypted: boolean;
}

/** Optional driver features; see `platypus_core::Capabilities`. */
export interface Capabilities {
  transactions: boolean;
  cancel: boolean;
  explain: boolean;
  object_ddl: boolean;
  multiple_databases: boolean;
  metadata_search: boolean;
  server_sessions: boolean;
  cancel_server_queries: boolean;
  /** The costliest statements and the use of tables and indexes of the admin tab. */
  server_statistics: boolean;
  /** What "Maintenance" runs on a table; empty for none. */
  maintenance: Maintenance[];
  /** The driver loads rows natively (PostgreSQL `COPY`, MySQL `LOAD DATA LOCAL`). */
  bulk_load: boolean;
}

/** A session of the server; see `platypus_core::ServerSession`. */
export interface ServerSession {
  id: string;
  user: string | null;
  database: string | null;
  client: string | null;
  application: string | null;
  state: string | null;
  started: string | null;
  query: string | null;
  /** Seconds the statement has run; `null` when none runs. */
  query_seconds: number | null;
  wait: string | null;
  blocked_by: string[];
  /** The session that asked (the navigator's). */
  current: boolean;
}

/** A lock held or waited for; see `platypus_core::ServerLock`. */
export interface ServerLock {
  session: string;
  kind: string;
  object: string | null;
  mode: string;
  granted: boolean;
}

export type EndSession = "cancel" | "terminate";

/** A statement the server has run, summed over its runs; see `platypus_core::ServerQuery`. */
export interface ServerQuery {
  query: string;
  /** One run with its values (MySQL 8), which can be explained as it is. */
  sample: string | null;
  database: string | null;
  user: string | null;
  calls: number;
  total_seconds: number;
  mean_seconds: number;
  max_seconds: number | null;
  rows: number | null;
}

/** How a table or an index has been used; see `platypus_core::ObjectStatistics`. */
export interface ObjectStatistics {
  schema: string;
  name: string;
  kind: "table" | "index";
  /** The table of an index. */
  table: string | null;
  size_bytes: number | null;
  rows: number | null;
  dead_rows: number | null;
  free_bytes: number | null;
  full_scans: number | null;
  index_scans: number | null;
  last_vacuum: string | null;
  last_analyze: string | null;
  /** A unique or primary key index. */
  unique: boolean;
}

/** A backup's or restore's progress and outcome; see `commands::backup::BackupEvent`. */
export type BackupEvent =
  | { kind: "progress"; step: string; done: number; total: number | null; bytes: boolean }
  | { kind: "finished"; bytes: number; elapsed_ms: number; output: string }
  | { kind: "failed"; message: string; output: string };

/** PostgreSQL's custom archive or SQL; MySQL writes SQL either way. */
export type BackupFormat = "custom" | "plain";

export interface BackupRequest {
  path: string;
  format: BackupFormat;
  schema?: string | null;
  structure_only: boolean;
}

export interface RestoreRequest {
  path: string;
  /** Drops the archive's objects before creating them again (`pg_restore --clean`). */
  clean: boolean;
}

/** The formats of a scheduled export. */
export type ScheduleExportFormat = "csv" | "json" | "parquet" | "xlsx";

/** What a scheduled task does; see `scheduler::plan::Task`. */
export type ScheduleTask =
  | { kind: "script"; path: string }
  | { kind: "export"; sql: string; format: ScheduleExportFormat }
  | { kind: "backup"; format: BackupFormat; schema?: string | null }
  | { kind: "profile"; profile_id: string };

/** How often a task runs; days of the week are 1 (Monday) to 7. */
export type ScheduleRepeat = { every: "hour" } | { every: "day" } | { every: "week"; days: number[] } | { every: "month"; day: number };

/** A scheduled task; see `scheduler::plan::Schedule`. */
export interface Schedule {
  id: string;
  name: string;
  enabled: boolean;
  connection_id: string;
  database: string | null;
  task: ScheduleTask;
  repeat: ScheduleRepeat;
  /** `HH:MM`, in `timezone` (empty: the system's). */
  time: string;
  timezone: string;
  folder: string | null;
  /** Files kept in the folder, the newest; 0 keeps all. */
  keep: number;
  retries: number;
  retry_minutes: number;
}

export interface ScheduleRun {
  started: string;
  finished: string;
  ok: boolean;
  message: string;
  file: string | null;
  attempt: number;
  manual: boolean;
}

/** A task with when it runs next and its last runs, the newest first. */
export interface ScheduleView extends Schedule {
  next: string | null;
  runs: ScheduleRun[];
  running: boolean;
}

/** The upkeep of a table; see `platypus_core::Maintenance`. */
export type Maintenance = "vacuum" | "analyze" | "reindex" | "optimize" | "check";

/** One step of a query plan; see `platypus_core::PlanNode`. Rows and times are per loop. */
export interface PlanNode {
  operation: string;
  detail: string | null;
  cost: number | null;
  rows: number | null;
  actual_rows: number | null;
  actual_ms: number | null;
  loops: number | null;
  /** The step's own cost, without its children's. */
  self_cost: number | null;
  /** With analyze, the milliseconds spent in the step itself over all its loops. */
  self_ms: number | null;
  /** Set when the step is a filtered full scan of a table. */
  full_scan: FullScan | null;
  properties: [string, string][];
  children: PlanNode[];
}

/** A step that reads a whole table to keep the rows matching `filter`. */
export interface FullScan {
  table: string;
  filter: string;
  /** Over all loops; without analyze an estimate, which PostgreSQL does not give. */
  rows_read: number | null;
}

export interface Explain {
  format: "json" | "text";
  /** The plan as the server returned it. */
  plan: string;
  root: PlanNode;
  analyzed: boolean;
  /** Filtered full scans of large tables, where an index may help. */
  suggestions: FullScan[];
}

/** Whose plan a pasted document is; see `commands/explain.rs`. */
export type PlanSource = "postgres" | "mysql" | "sqlite" | "sqlserver";

/** A plan read from a document, and whose it was taken to be. */
export interface ReadPlan {
  explain: Explain;
  source: PlanSource;
}

/** A request the app sent to a database; see `querylog.rs`. */
export interface LogEntry {
  id: number;
  rev: number;
  connection: string;
  kind: "query" | "metadata" | "transaction";
  text: string;
  /** Milliseconds since the Unix epoch. */
  started_at: number;
  elapsed_ms: number | null;
  /** Until the server's first answer to a statement: its work plus the network. */
  first_ms: number | null;
  /** The connection's last keep-alive round trip: about the network's share. */
  network_ms: number | null;
  rows: number | null;
  status: "running" | "fetching" | "done" | "closed" | "failed";
  error: string | null;
}

export interface CommandError {
  kind: string;
  message: string;
  code: string | null;
  detail: string | null;
  hint: string | null;
  position: number | null;
}

/** A driver downloaded from the driver registry. */
export interface InstalledDriver {
  /** Its name in the registry, e.g. `snowflake`. */
  name: string;
  title: string;
  version: string;
  license: string;
  library: string;
}

/** A driver package the registry offers for this platform. */
export interface DriverPackage {
  name: string;
  title: string;
  version: string;
  license: string;
  url: string;
  sha256: string | null;
}

export interface DriverStatus {
  installed: InstalledDriver | null;
  /** `null` when the registry cannot be reached, with `error` saying why. */
  package: DriverPackage | null;
  error: string | null;
}

export type DriverInstallEvent = { kind: "progress"; done: number; total: number | null };

/** The databases whose driver is downloaded when needed (ADBC). */
export const DOWNLOADED_DRIVERS: readonly DriverKind[] = ["snowflake", "bigquery", "databricks", "trino"];

/** The databases whose vendor library the app installs: Oracle Instant Client, IBM's ODBC driver, DuckDB. */
export const VENDOR_DRIVERS: readonly DriverKind[] = ["oracle", "db2", "duckdb"];

/** A vendor's client library, as the app would download it here. */
export interface VendorPackage {
  name: string;
  title: string;
  version: string;
  license: string;
  license_url: string;
  /** The vendor's download page. */
  vendor_url: string;
  archives: { url: string; sha256: string; size: number }[];
  library: string;
  /** Registers an ODBC driver with the system, which Windows asks administrator rights for. */
  register: { path: string; args: string[] } | null;
  unregister: { path: string; args: string[] } | null;
}

export interface VendorStatus {
  /** `null` where the app installs nothing on this system. */
  package: VendorPackage | null;
  installed: InstalledDriver | null;
  /** The ODBC driver the system has for the database (Db2), wherever it came from. */
  system_driver: string | null;
  ready: boolean;
}

export type VendorInstallEvent = { kind: "progress"; done: number; total: number | null } | { kind: "registering" };

/** The ODBC drivers and data sources (DSNs) of the system. */
export interface OdbcSystem {
  /** Whether this build reaches ODBC drivers (Windows). */
  available: boolean;
  drivers: { name: string; path: string | null }[];
  data_sources: { name: string; driver: string }[];
}

/** What a quick filter on a result column names in the WHERE: the column it shows, as SQL; or
 *  none, for a computed column or one of `*` whose table cannot be told. */
export type FilterTarget = { kind: "column"; sql: string } | { kind: "computed" } | { kind: "ambiguous" };

export interface ColumnInfo {
  name: string;
  db_type: string;
  numeric: boolean;
  /** Bytes, shown as hex. */
  binary: boolean;
}

export type QueryEvent =
  | { type: "started"; columns: ColumnInfo[]; elapsed_ms: number }
  | { type: "progress"; rows: number; fetching: boolean }
  | { type: "finished"; rows: number; elapsed_ms: number; complete: boolean; stopped: boolean; more: boolean }
  | { type: "affected"; rows: number; elapsed_ms: number }
  | { type: "failed"; error: CommandError }
  /** The statement's next result (a T-SQL batch, a MySQL CALL): the events after this one are
   *  about job `job_id`. */
  | { type: "next"; job_id: number }
  /** What the server said meanwhile: notices, PRINT, warnings. */
  | { type: "messages"; lines: string[] };

export interface UnfilteredTable {
  schema: string | null;
  name: string;
  /** `UPDATE`, `DELETE`, `TRUNCATE`, or mongosh's `updateMany`, `deleteMany`. */
  verb: string;
  /** SQL Server's `TOP (n)`: up to that many rows the database picks, not every one. */
  limited: boolean;
}

/** Offsets are UTF-16 code units, like JavaScript string indices. */
export interface StatementSpan {
  start: number;
  end: number;
  read_only: boolean;
  /** `command` is a client command such as `@set x = 1`, run by the app. */
  kind: "query" | "dml" | "ddl" | "other" | "command";
  /** The tables an UPDATE or DELETE without WHERE, or a TRUNCATE, changes in every row:
   *  several for `TRUNCATE a, b` or a SQL Server batch. */
  unfiltered: UnfilteredTable[];
  /** `:name`, `?` and `${name}` placeholders, in the same units as `start`. */
  parameters: ParameterSpan[];
}

export interface ParameterSpan {
  /** `named` and `positional` take literals, `variable` (`${name}`) text as typed. */
  kind: "named" | "positional" | "variable";
  /** Empty for `?`. */
  name: string;
  start: number;
  end: number;
}

/** What the linter found in the editor's SQL; offsets in UTF-16 units. */
export interface SqlLint {
  issues: { kind: "select_star" | "join_without_condition"; from: number; to: number }[];
  statements: {
    /** `name` is `null` for subqueries, functions and common table expressions. */
    tables: { alias: string; schema: string | null; name: string | null }[];
    columns: { qualifier: string; column: string; from: number; to: number }[];
  }[];
}

/** A saved script or folder, by its path relative to the project's scripts folder. */
export interface ScriptEntry {
  path: string;
  folder: boolean;
  connection: string | null;
}

export interface ScriptList {
  /** The project's scripts folder, absolute. */
  root: string;
  entries: ScriptEntry[];
}

export interface CliOutput {
  command: string;
  status: number | null;
  output: string;
  elapsed_ms: number;
}

export interface SqlAnalysis {
  statements: StatementSpan[];
  at_cursor: number | null;
  /** Statements a script sends together (T-SQL batches up to GO), as `first..end` indices
   *  into `statements`, run `repeat` times (`GO 3`); empty when each runs alone. */
  batches: { first: number; end: number; repeat: number }[];
}

export type HistoryStatus = "ok" | "error" | "cancelled";

/** A call an agent made to one of the MCP server's tools. */
export interface AgentCall {
  id: number;
  /** The server's run it belongs to. */
  session: string;
  agent: string;
  tool: string;
  connection: string | null;
  /** The arguments, as JSON text. */
  arguments: string;
  /** Unix epoch milliseconds. */
  started_at: number;
  duration_ms: number;
  status: HistoryStatus;
  error: string | null;
}

export interface AgentCallQuery {
  text: string | null;
  session: string | null;
  status: HistoryStatus | null;
  before_id: number | null;
  limit: number;
}

/** A run of the MCP server, with how many calls it answered. */
export interface AgentSession {
  session: string;
  agent: string;
  first_at: number;
  last_at: number;
  calls: number;
  errors: number;
}

export type AgentInstallState = "missing" | "installed" | "different" | "unreadable";

/** An MCP client PlatypusDB can be installed in. */
export interface AgentClient {
  id: string;
  name: string;
  /** Its configuration file. */
  path: string;
  format: "json" | "toml";
  exists: boolean;
  state: AgentInstallState;
  /** The entry to add by hand, in the file's format. */
  snippet: string;
}

/** SQL an agent proposed, to open in an editor tab. */
export interface AgentProposal {
  id: number;
  agent: string;
  connection_id: string;
  title: string | null;
  sql: string;
  created_at: number;
}

export interface InstalledClient {
  path: string;
  backup: string | null;
}

export interface HistoryEntry {
  id: number;
  connection_id: string;
  sql: string;
  /** Unix epoch milliseconds. */
  started_at: number;
  duration_ms: number;
  rows: number | null;
  status: HistoryStatus;
  error: string | null;
  /** The agent that ran it, e.g. `claude-code`; `null` when run in the app. */
  origin: string | null;
}

/** Who ran the statements the history shows: the app, or agents. */
export type HistoryOrigin = "app" | "agent";

export interface SchemaMeta {
  name: string;
  system: boolean;
  current: boolean;
}

export type ObjectKind =
  | "table"
  | "view"
  | "materialized_view"
  | "foreign_table"
  | "sequence"
  | "function"
  | "procedure"
  | "trigger"
  | "type"
  | "domain"
  | "extension"
  | "event"
  | "dictionary"
  | "synonym"
  | "package";

export interface ObjectMeta {
  name: string;
  kind: ObjectKind;
  comment: string | null;
  /** The table of a trigger, or the partitioned table of a partition. */
  parent: string | null;
  /** Estimated rows of a table, from the statistics. */
  rows: number | null;
  /** Bytes on disk of a table with its indexes. */
  size: number | null;
}

export interface MetadataMatch {
  schema: string;
  name: string;
  kind: ObjectKind;
  parent: string | null;
  matched: "name" | "column" | "source";
  column: string | null;
}

/** Rows of a table that hold the searched text, at most `limit`. */
export interface TableHits {
  rows: number;
  limit: number;
  /** The WHERE condition that selects them. */
  condition: string;
}

export interface ColumnMeta {
  name: string;
  data_type: string;
  nullable: boolean;
  default: string | null;
  primary_key: boolean;
  comment: string | null;
  /** Values an enum column accepts; empty for other types. */
  enum_values: string[];
  /** Fields of a composite type, or of an array's composite elements (PostgreSQL). */
  fields: FieldMeta[];
}

export interface FieldMeta {
  name: string;
  data_type: string;
}

export interface IndexMeta {
  name: string;
  columns: string[];
  unique: boolean;
  primary: boolean;
  /** Covers only the rows of a condition (`WHERE`). */
  partial: boolean;
  size: number | null;
}

export interface ForeignKeyMeta {
  name: string;
  columns: string[];
  ref_schema: string;
  ref_table: string;
  ref_columns: string[];
}

export interface ConstraintMeta {
  name: string;
  kind: "primary_key" | "unique" | "check" | "exclusion";
  columns: string[];
  definition: string;
}

/** A foreign key of another table that points at this one. */
export interface TableDetails {
  columns: ColumnMeta[];
  indexes: IndexMeta[];
  foreign_keys: ForeignKeyMeta[];
  constraints: ConstraintMeta[];
  /** Foreign keys of other tables that point at this one. */
  referenced_by: ReferencingKey[];
}

export interface Property {
  name: string;
  value: string;
}

/** An object as the backend identifies it. */
export interface ObjectTarget {
  schema: string;
  name: string;
  kind: ObjectKind;
  parent: string | null;
}

/** What to generate SQL for; columns and keys apply to DML. */
export interface GenerateTarget {
  /** `null` for the session's current schema. */
  schema: string | null;
  name: string;
  kind: ObjectKind;
  parent: string | null;
  columns?: string[];
  keys?: string[];
}

export type GenerateRequest =
  | { statement: "select"; limit: number | null }
  | { statement: "insert" | "update" | "delete" | "upsert" | "truncate" }
  | { statement: "drop"; cascade: boolean }
  | { statement: "rename"; new_name: string };

export interface DialectSupport {
  cascade: boolean;
  droppable: ObjectKind[];
  renamable: ObjectKind[];
}

/** A table as the table editor edits it. Each part keeps its name in the database in
 *  `original` (`null` for a new part), so a rename is not taken for a new part. */
export interface TableDesign {
  name: string;
  columns: ColumnDesign[];
  /** The name of the primary key constraint; its columns are those marked `primary_key`. */
  primary_key_name?: string | null;
  indexes: IndexDesign[];
  foreign_keys: ForeignKeyDesign[];
  /** UNIQUE and CHECK constraints. */
  constraints: ConstraintDesign[];
  /** SQLite's `WITHOUT ROWID`, `STRICT`, kept from the database. */
  options?: string | null;
  /** The table's `CREATE TRIGGER` statements (SQLite), run again when it is rebuilt. */
  triggers?: string[];
}

export interface ColumnDesign {
  original: string | null;
  name: string;
  data_type: string;
  nullable: boolean;
  /** An SQL expression. */
  default: string | null;
  primary_key: boolean;
  comment: string | null;
  /** Kept from the database, e.g. MySQL's `auto_increment`. */
  extra?: string | null;
  /** `GENERATED ALWAYS AS (…) VIRTUAL` of a generated column, kept from the database. */
  generated?: string | null;
  /** A collation other than the default one, kept from the database. */
  collation?: string | null;
}

export interface IndexDesign {
  original: string | null;
  name: string;
  columns: string[];
  unique: boolean;
  /** Its own `CREATE INDEX` (SQLite), run again while the index is unchanged. */
  sql?: string | null;
}

export interface ForeignKeyDesign {
  original: string | null;
  name: string;
  columns: string[];
  ref_schema: string;
  ref_table: string;
  ref_columns: string[];
  /** `null` for NO ACTION. */
  on_delete: string | null;
  on_update: string | null;
}

export interface ConstraintDesign {
  original: string | null;
  name: string;
  kind: "unique" | "check";
  columns: string[];
  /** The condition of a CHECK, without `CHECK`. */
  check: string;
}

/** The statements that change a table, sent back as is to run them. */
export interface TableScript {
  before: string[];
  statements: string[];
  after: string[];
  transactional: boolean;
  warnings: string[];
}

/** A table of a schema with its details (ER diagram, linter). */
export interface SchemaTable {
  name: string;
  comment: string | null;
  details: TableDetails;
  /** Why the details could not be read; they are empty then. */
  error: string | null;
}

export type LintKind =
  | "no_primary_key"
  | "unindexed_foreign_key"
  | "duplicate_index"
  | "redundant_index"
  | "foreign_key_type_mismatch"
  | "unreadable";

/** What the schema linter found; `fix` is the statement that fixes it, to review first. */
export interface LintFinding {
  kind: LintKind;
  table: string;
  /** The index or foreign key concerned; `null` for the table itself. */
  object: string | null;
  /** In English; `template` with `values` is the same text, to translate. */
  message: string;
  template: string;
  values: Record<string, string>;
  fix: string | null;
}

/** A foreign key of the catalog: `schema.table (columns)` references `ref_*`. */
export interface CatalogForeignKey {
  schema: string;
  table: string;
  columns: string[];
  ref_schema: string;
  ref_table: string;
  ref_columns: string[];
}

/** Tables and columns by schema, and the foreign keys between them, for autocompletion. */
export interface CompletionCatalog {
  default_schema: string | null;
  schemas: Record<string, Record<string, string[]>>;
  foreign_keys: CatalogForeignKey[];
}

export interface FormatSettings {
  keyword_case: "upper" | "lower" | "preserve";
  /** Spaces per level. */
  indent: number;
}

export interface TransactionState {
  autocommit: boolean;
  open: boolean;
}

/** A value the data editor writes: typed text, NULL or the column default. */
export type CellInput = { kind: "value"; text: string } | { kind: "null" } | { kind: "default" };

export interface ColumnValue {
  /** Result column index. */
  column: number;
  value: CellInput;
}

/** How the database fills a column by itself: numbered when left out (serials,
 *  `AUTO_INCREMENT`, SQLite's rowid), numbered and taking no value (`GENERATED ALWAYS AS
 *  IDENTITY`, SQL Server's `IDENTITY`), or computed (generated columns, `rowversion`). */
export type Generated = "numbered" | "identity" | "computed";

export interface GeneratedColumn {
  name: string;
  kind: Generated;
}

/** Pending changes to a result of one table; see `commands/data.rs`. */
export interface TableChanges {
  table: { schema: string; name: string };
  /** Result columns that identify a row. */
  key: number[];
  /** The key is the table's primary or unique key, not a virtual key: deleted rows are
   *  removed in batches. */
  unique_key: boolean;
  /** Result columns that are the table's own, not expressions. */
  table_columns: number[];
  /** Default expressions by result column. */
  defaults: (string | null)[];
  /** How the database fills each result column by itself, if it does. */
  generated: (Generated | null)[];
  updates: { row: number; values: ColumnValue[] }[];
  inserts: ColumnValue[][];
  deletes: number[];
}

/** An item of a query's select list; see `commands/sql.rs`. */
export type SourceItem = { kind: "all" } | { kind: "column"; name: string } | { kind: "other" };

/** The one table a query reads; `schema` is `null` when left to the session. */
export interface QuerySource {
  schema: string | null;
  name: string;
  items: SourceItem[];
}

/** A table a foreign key points to: its key column and the column describing a row. */
export interface Lookup {
  schema: string;
  table: string;
  key: string;
  label: string;
}

/** A foreign key of another table that points at a table. */
export interface ReferencingKey {
  schema: string;
  table: string;
  name: string;
  columns: string[];
  /** The referenced table's columns they hold. */
  ref_columns: string[];
}

/** Totals of a selection; the number ones cover the number columns only. */
export interface SelectionStats {
  cells: number;
  values: number;
  numbers: number;
  sum: number | null;
  avg: number | null;
  min: number | null;
  max: number | null;
  /** `sum`, `min` and `max` as exact decimal text, when every number is an integer or a
   *  decimal of one scale (the numbers above round integers beyond 2^53). */
  sum_exact: string | null;
  min_exact: string | null;
  max_exact: string | null;
}

/** Statistics of one column over selected rows, for the calc panel. */
export interface ColumnStats {
  column: number;
  values: number;
  nulls: number;
  distinct: number;
  /** For number columns. */
  numbers: {
    sum: number;
    avg: number;
    min: number;
    max: number;
    median: number;
    stddev: number | null;
    /** As in `SelectionStats`. */
    sum_exact: string | null;
    min_exact: string | null;
    max_exact: string | null;
  } | null;
  /** Smallest and largest text of other columns. */
  first: string | null;
  last: string | null;
}

/** A geometry in plane coordinates, as `platypus_core::geometry::Shape`. */
export type Shape =
  | { type: "point"; x: number; y: number }
  | { type: "line"; points: [number, number][] }
  /** The outer ring first, then the holes. */
  | { type: "polygon"; rings: [number, number][][] }
  | { type: "circle"; x: number; y: number; r: number }
  | { type: "collection"; items: Shape[] };

/** The geometries of loaded cells, for the map panel. */
export interface Geometries {
  shapes: { row: number; column: number; shape: Shape }[];
  /** Cells that are not NULL and hold no geometry. */
  unreadable: number;
  truncated: boolean;
}

export interface GroupCounts {
  groups: { values: (string | null)[]; count: number }[];
  rows: number;
  distinct: number;
}

/** Clipboard formats of `copyRows`. */
export type CopyFormat = "tsv" | "csv" | "json" | "markdown" | "html" | "insert" | "in_list";

export interface SearchResult {
  /** `[row, column]` of the matching cells, row by row. */
  matches: [number, number][];
  /** More cells matched than were reported. */
  truncated: boolean;
}

export type ExportFormat = "csv" | "json" | "ndjson" | "parquet" | "xlsx" | "sql" | "markdown" | "html" | "xml";

export interface ExportOptions {
  format: ExportFormat;
  delimiter: string;
  header: boolean;
  /** SQL: the table the INSERT statements write to. */
  table?: string;
  /** SQL: the database the statements are written for. */
  driver?: DriverKind;
  compression?: Compression;
  /** A signed URL (S3, GCS) the finished file is also sent to. */
  upload_url?: string | null;
  /** A bucket the finished file is also sent to, signed with its keys. */
  bucket?: Bucket | null;
}

/** A bucket of S3 or a service with its API (GCS with HMAC keys, R2, MinIO), as `s3::Bucket`. */
export interface Bucket {
  /** e.g. `https://s3.eu-west-1.amazonaws.com`. */
  endpoint: string;
  /** `auto` for GCS and R2. */
  region: string;
  bucket: string;
  /** The folder of the files in the bucket. */
  prefix: string;
  access_key: string;
  secret_key: string;
}

export type Compression = "none" | "gzip" | "zstd";

export type ExportEvent =
  | { type: "progress"; rows: number; /** Bytes of a file read, and its size. */ bytes?: [number, number] }
  | { type: "done"; rows: number; elapsed_ms: number; warnings?: string[] }
  | { type: "failed"; error: CommandError }
  | { type: "cancelled" };

/** Extensions of the files that open as a database and import into a table. */
export const DATA_FILE_EXTENSIONS = ["csv", "tsv", "json", "ndjson", "jsonl", "parquet", "xlsx", "xlsm", "xls", "ods"];

/** Whether `path` names a data file, which opens as an in-memory DuckDB database on it. */
export const isDataFile = (path: string) => DATA_FILE_EXTENSIONS.includes(path.split(".").pop()?.toLowerCase() ?? "");

/** Whether `c` opens a data file: its database lives in memory, so a copy there would be lost. */
export const opensDataFile = (c: SavedConnection) => c.driver === "duckdb" && isDataFile(c.database);

export interface ImportPreview {
  /** The tables of the file: one, or a workbook's sheets. */
  tables: string[];
  table: string;
  /** For a source table, also what a new table keeps of each column: NOT NULL, the primary
   *  key and a default the target reads. */
  columns: { name: string; source_type: string; target_type: string; not_null?: boolean; primary_key?: boolean; default?: string | null }[];
  rows: (string | null)[][];
}

/** How a CSV or TSV file is read: its encoding (`null` tells it from the bytes) and whether
 *  lines with another number of values are left out instead of failing. */
export interface FileOptions {
  encoding: string | null;
  skip_bad_lines: boolean;
}

/** What happens to the rows already in the table: kept, deleted first, or updated by key. */
export type ImportMode = "append" | "replace" | "upsert";

export interface ImportPlan {
  source_table: string;
  /** The schema of `source_table` when it is another database's. */
  source_schema?: string | null;
  schema: string | null;
  table: string;
  /** Create the table first, from the columns' `data_type`. */
  create: boolean;
  mode?: ImportMode;
  /** The target columns that identify a row, for an upsert. */
  keys?: string[];
  columns: { source: string; target: string; data_type: string | null; not_null?: boolean; default?: string | null }[];
  /** The primary key of a created table. */
  primary_key?: string[];
}

/** What a saved transfer profile does, with the choices of the dialog it was saved from; see
 *  `profiles::ProfileTask`. */
export type ProfileTask =
  | { kind: "export_query"; connection_id: string; database: string | null; sql: string; path: string; options: ExportOptions }
  | {
      kind: "export_tables";
      connection_id: string;
      database: string | null;
      tables: { schema: string | null; name: string }[];
      dir: string;
      options: ExportOptions;
    }
  | { kind: "import_file"; connection_id: string; database: string | null; path: string; plan: ImportPlan; options: FileOptions }
  | {
      kind: "transfer_table";
      source_connection_id: string;
      source_database: string | null;
      target_connection_id: string;
      target_database: string | null;
      plan: ImportPlan;
    }
  | {
      kind: "migrate";
      source_connection_id: string;
      source_database: string | null;
      target_connection_id: string;
      target_database: string | null;
      source_schema: string;
      target_schema: string;
      /** The tables to copy; `null` is the whole schema, read when the profile runs. */
      tables: string[] | null;
      objects: boolean;
    };

/** A named transfer that runs again by hand or from a scheduled task. */
export interface TransferProfile {
  id: string;
  name: string;
  task: ProfileTask;
}

/** A set of connections worked on together; a connection can be in several. */
export interface Project {
  id: string;
  name: string;
  /** `#rrggbb`. */
  color: string;
  /** One or two characters on the project's square. */
  initials: string;
  connections: string[];
  /** Folders of the project's connections; absent when there are none. */
  folders?: ConnectionFolder[];
  /** The folder of each connection not at the root, by connection id. */
  placement?: Record<string, string>;
}

/** A folder of connections inside a project. */
export interface ConnectionFolder {
  id: string;
  name: string;
  /** Absent at the project's root. */
  parent?: string | null;
}

/** What a note or tag is on: `database` empty for the connection's own, `column` for the object. */
export interface CatalogKey {
  database: string;
  schema: string;
  object: string;
  column: string;
}

/** A local note and tags, never written to the database. */
export interface CatalogEntry {
  key: CatalogKey;
  note: string;
  tags: string[];
}

export interface TagCount {
  tag: string;
  count: number;
}

/** The column picked to describe a table's rows beside the foreign keys to it; `key.column` is empty. */
export interface DisplayColumn {
  key: CatalogKey;
  column: string;
}

export type DocsFormat = "markdown" | "html";

/** What the schema documentation covers and where it is written. */
export interface DocsRequest {
  title: string;
  schemas: string[];
  views: boolean;
  diagrams: boolean;
  notes: boolean;
  format: DocsFormat;
  path: string;
}

export type DocsEvent =
  | { kind: "progress"; done: number; total: number }
  | { kind: "finished"; path: string }
  | { kind: "failed"; message: string };

/** A folder of projects in the rail; a project is in one at most. */
export interface ProjectFolder {
  id: string;
  name: string;
  /** `#rrggbb`. */
  color: string;
  projects: string[];
  collapsed: boolean;
}

/** One project read from another tool's file; `name` is null for files of one set of
 *  connections. */
export interface ImportedProject {
  name: string | null;
  /** Each folder as its names from the root, parents first. */
  folders: string[][];
  connections: SavedConnection[];
  /** The folder of each connection not at the root, by connection id. */
  placement: Record<string, string[]>;
  /** The connections of the file that are not imported. */
  skipped: SkippedConnection[];
  /** What a connection needs before it can connect, by connection id. */
  notes: Record<string, ImportNote[]>;
  /** What to know about the project as a whole. */
  project_notes: ProjectNote[];
  /** The connections that come with a saved password; the passwords stay in the backend. */
  with_password: string[];
}

/** A connection of the file to a database the import does not read, with the tool's own
 *  ids of the database and its driver. */
export interface SkippedConnection {
  name: string;
  provider: string;
  driver: string;
}

/** Windows or Entra ID logins, which the SQL Server driver does not do, a named instance
 *  without a port, and a database file that is not there. */
export type ImportNote =
  | { kind: "windows_auth" }
  | { kind: "entra_auth" }
  | { kind: "named_instance"; instance: string }
  | { kind: "missing_file"; path: string };

/** About a pasted URL or connection string: the notes of an import, and the parameters it
 *  left out (by name). */
export type ParseNote = ImportNote | { kind: "ignored"; keys: string[] } | { kind: "other_hosts"; hosts: string[] };

/** The connection form's fields read from a pasted URL or connection string. */
export interface ParsedConnection {
  driver: DriverKind;
  host: string;
  port: number | null;
  database: string;
  user: string;
  password: string | null;
  ssl_mode: SslMode;
  ssl_root_cert: string | null;
  ssl_client_cert: string | null;
  ssl_client_key: string | null;
  options: Record<string, string>;
  notes: ParseNote[];
}

/** DBeaver keeps no saved logins for the project, or they could not be read. */
export type ProjectNote = { kind: "no_saved_logins" } | { kind: "saved_logins_unreadable" };

export type ImportInto =
  | { kind: "new_project"; name: string }
  | { kind: "existing"; id: string }
  | { kind: "none" };

export interface ImportRequest {
  targets: { project: ImportedProject; into: ImportInto }[];
  /** A rail folder with this name holds the new projects. */
  rail_folder: string | null;
  /** Saves the passwords read with the file; otherwise each connection asks on first use. */
  passwords: boolean;
}

/** Connections, projects and rail folders, after a change that touches more than one. */
export interface Workspace {
  connections: SavedConnection[];
  projects: Project[];
  project_folders: ProjectFolder[];
}

/** The sample database, opened: its connection and the first of its scripts. */
export interface SampleDatabase {
  workspace: Workspace;
  connection: string;
  script: string;
}

export interface AppInfo {
  name: string;
  version: string;
  /** The commit and date the app is built from, e.g. "82678c0 · 2026-10-27 16:52". */
  build: string | null;
  /** Developer tools are open, as for the end-to-end tests: the page gets test hooks. */
  devtools: boolean;
  /** A portable copy: its data is in the `data` folder next to it, and it updates by
   *  replacing its executable. */
  portable: boolean;
  os: string;
  arch: string;
}

/** Normalizes anything thrown by `invoke` into a `CommandError`. */
/** Puts the backend's own messages in the interface's language (`storeErrors.ts`, set at
 *  start: importing it here would import this module back through the settings). */
let appMessage = (message: string) => message;
export function translateAppMessages(translate: (message: string) => string) {
  appMessage = translate;
}

/** The error a command failed with; the app's own messages in the interface's language, the
 *  databases' as they are. */
export function toCommandError(e: unknown): CommandError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) {
    const error = e as CommandError;
    return error.kind === "database" ? error : { ...error, message: appMessage(error.message) };
  }
  return { kind: "other", message: String(e), code: null, detail: null, hint: null, position: null };
}

// Analysis (group M): column statistics

/** A result column at a glance, over the rows loaded (`column_profile`). */
export interface ColumnProfile {
  rows: number;
  nulls: number;
  distinct: number;
  /** As shown; by value for numbers and dates, by text otherwise; `null` for binary, JSON
   *  and composite columns. */
  min: string | null;
  max: string | null;
  /** Numbers and dates: 20 equal ranges. */
  histogram: { from: string; to: string; count: number }[];
  /** Text: the 5 most frequent values. */
  top: { value: string; count: number }[];
  /** Over every row of the result, not only the loaded ones. */
  complete: boolean;
}

// Analysis (group M): charts
/** What a chart draws, columns by position (`analysis/chartConfig.ts` makes it). */
export interface ChartRequest {
  kind: "bar" | "line" | "scatter" | "pie";
  x: number;
  y: number[];
  aggregate: "sum" | "count" | "avg" | "min" | "max";
}

/** A chart's data over the loaded rows. */
export interface ChartData {
  /** Bars and pie use `categories` and `values`; lines and scatter `points`, X as a number,
   *  milliseconds since the epoch or a category's position. */
  x_axis: "category" | "number" | "time";
  /** `null` is the category of SQL NULL, apart from the text "NULL". */
  categories: (string | null)[];
  /** The last category gathers the ones left out. */
  other: boolean;
  /** A value is `null` where a category has no number to aggregate (no bar, not 0). */
  series: { name: string; values: (number | null)[]; points: [number, number][] }[];
  rows: number;
  /** Points were reduced or categories gathered to keep the chart light. */
  reduced: boolean;
}

// Analysis (group M): SQL on results
/** A table of the in-memory database of "Query with SQL" and the result it copies. */
export interface ResultTable {
  job_id: number;
  table: string;
}

/** Why a table of "Query with SQL" lacks rows of its result: reading stopped at the memory
 *  limit, or for another reason (an error, a cancel, a script's row limit). */
export type ResultPartial = "memory_limit" | "stopped";

/** The temporary DuckDB connection of "Query with SQL"; `partial` when a table lacks rows. */
export interface OpenedResultDb {
  connection: SavedConnection;
  partial: ResultPartial | null;
}

// Analysis (group M): compare with the previous run
/** A row of a diff: as it is now (a removed row as it was); a changed row has the columns that
 *  differ with their previous value. */
export interface DiffRow {
  change: "added" | "removed" | "changed";
  values: (string | null)[];
  before: [number, string | null][];
}

/** What changed since the previous run of a statement. */
export interface Diff {
  columns: string[];
  added: number;
  removed: number;
  changed: number;
  /** Rows matched by the table's key; otherwise by all their values. */
  keyed: boolean;
  /** At most 10,000; the counts go on. */
  rows: DiffRow[];
  truncated: boolean;
  /** The rows each run had loaded. */
  before_rows: number;
  after_rows: number;
  /** Both runs were loaded in full. Otherwise added rows are counted only when the previous
   *  run was complete, removed rows only when the new one is. */
  complete: boolean;
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  diagnosticsInfo: () => invoke<DiagnosticsInfo>("diagnostics_info"),
  dismissCrash: () => invoke<void>("dismiss_crash"),
  logError: (message: string) => invoke<void>("log_error", { message }),
  saveDiagnostics: (path: string) => invoke<void>("save_diagnostics", { path }),
  agentSetup: () => invoke<AgentSetup>("agent_setup"),
  agentBlockers: () => invoke<Record<string, AgentBlocker>>("agent_blockers"),
  agentBridge: () => invoke<BridgeStatus>("agent_bridge"),
  /** Starts the bridge again after its settings changed. */
  restartAgentBridge: () => invoke<BridgeStatus>("restart_agent_bridge"),
  regenerateAgentToken: () => invoke<BridgeStatus>("regenerate_agent_token"),
  /** Answers a change an agent asked for; `false` when it no longer waits. */
  answerAgentWrite: (id: string, verdict: Verdict) => invoke<boolean>("answer_agent_write", { id, verdict }),
  /** The changes waiting for an answer, the oldest first, with how long each has waited. */
  agentWritesWaiting: () => invoke<{ request: WriteRequest; waited_ms: number }[]>("agent_writes_waiting"),
  connectedAgents: () => invoke<ConnectedAgent[]>("connected_agents"),
  staleAgentRegistrations: () => invoke<StaleRegistration[]>("stale_agent_registrations"),
  agentClients: () => invoke<AgentClient[]>("agent_clients"),
  installAgentClient: (id: string) => invoke<InstalledClient>("install_agent_client", { id }),
  agentCalls: (query: AgentCallQuery) => invoke<AgentCall[]>("agent_calls", { query }),
  agentSessions: (limit: number) => invoke<AgentSession[]>("agent_sessions", { limit }),
  agentCallsExport: (query: AgentCallQuery) => invoke<string>("agent_calls_export", { query }),
  clearAgentCalls: () => invoke<void>("clear_agent_calls"),
  takeAgentProposals: () => invoke<AgentProposal[]>("take_agent_proposals"),
  listConnections: () => invoke<SavedConnection[]>("list_connections"),
  /** Opens a window for a tab (as `persistTab` saves it); the window's label. */
  openTabWindow: (tab: string, title: string) => invoke<string>("open_tab_window", { tab, title }),
  /** The tab that waits for the window `label`, once. */
  takeWindowTab: (label: string) => invoke<string | null>("take_window_tab", { label }),
  openQuickWindow: (title: string) => invoke<void>("open_quick_window", { title }),
  /** Shows the tray icon with its menu (quick query, show, quit, then the quick window's
   *  title); `null` removes it. */
  setTray: (labels: [string, string, string, string] | null) => invoke<void>("set_tray", { labels }),
  /** Builds the sample SQLite database if needed, with its connection (in `project` too)
   *  and its scripts. */
  openSampleDatabase: (project: string | null) => invoke<SampleDatabase>("open_sample_database", { project }),
  defaultConnection: (driver: DriverKind) =>
    invoke<SavedConnection>("default_connection", { driver }),
  /** Reads a URL (`postgres://…`), a JDBC URL or a `Server=…;Database=…` string; `driver`
   *  is the form's, which a string that names none is read for. */
  parseConnectionUrl: (input: string, driver: DriverKind | null = null) =>
    invoke<ParsedConnection>("parse_connection_url", { input, driver }),
  /** The URL of a saved connection, without its password. */
  connectionUrl: (connectionId: string) => invoke<string>("connection_url", { connectionId }),
  /** The URL of a form's settings, without a password; `null` when the driver has none. */
  formConnectionUrl: (connection: SavedConnection) => invoke<string | null>("form_connection_url", { connection }),
  /** Whether a password is saved for the connection (a locked one counts). */
  connectionHasPassword: (connectionId: string) => invoke<boolean>("connection_has_password", { connectionId }),
  /** Saves the password typed at the prompt of a connection that keeps it in the keychain. */
  rememberPassword: (connectionId: string, password: string) =>
    invoke<void>("remember_password", { connectionId, password }),
  /** Saves a copy of a connection, with its secrets, next to it. */
  duplicateConnection: (id: string) =>
    invoke<{ connection: SavedConnection; workspace: Workspace }>("duplicate_connection", { id }),
  /** `projects` replaces the projects the connection is in; `null` leaves them. */
  saveConnection: (
    connection: SavedConnection,
    password: PasswordInput,
    sshSecrets: SshSecrets = {},
    projects: string[] | null = null,
  ) => invoke<SavedConnection[]>("save_connection", { connection, password, sshSecrets, projects }),
  detectImports: () => invoke<ImportFile[]>("detect_imports"),
  /** The projects and connections in another tool's file; nothing is saved yet. */
  readImport: (source: ImportSource, path: string) => invoke<ImportedProject[]>("read_import", { source, path }),
  /** Saves what was picked from an import, all or nothing. */
  saveImported: (request: ImportRequest) => invoke<Workspace>("save_imported", { request }),
  /** Drops the passwords `readImport` keeps for `saveImported`. */
  forgetImport: () => invoke<void>("forget_import"),
  listProjects: () => invoke<Project[]>("list_projects"),
  saveProject: (project: Project) => invoke<Project[]>("save_project", { project }),
  /** Deletes a project; its connections stay. */
  deleteProject: (id: string) => invoke<Project[]>("delete_project", { id }),
  setProjectMembership: (connectionId: string, projectId: string, member: boolean) =>
    invoke<Project[]>("set_project_membership", { connectionId, projectId, member }),
  /** The initials a project gets from its name. */
  projectInitials: (name: string) => invoke<string>("project_initials", { name }),
  createConnectionFolder: (projectId: string, name: string, parent: string | null) =>
    invoke<Project[]>("create_connection_folder", { projectId, name, parent }),
  renameConnectionFolder: (projectId: string, folderId: string, name: string) =>
    invoke<Project[]>("rename_connection_folder", { projectId, folderId, name }),
  moveConnectionFolder: (projectId: string, folderId: string, parent: string | null) =>
    invoke<Project[]>("move_connection_folder", { projectId, folderId, parent }),
  /** Deletes a folder; what it holds moves up to its parent. */
  deleteConnectionFolder: (projectId: string, folderId: string) =>
    invoke<Project[]>("delete_connection_folder", { projectId, folderId }),
  /** Puts a connection into a folder of the project, or at its root with `null`. */
  placeConnection: (projectId: string, connectionId: string, folderId: string | null) =>
    invoke<Project[]>("place_connection", { projectId, connectionId, folderId }),
  /** Moves a connection into a folder of the project (`null`: its root), just before
   *  `before` or after the last connection there. */
  arrangeConnection: (projectId: string, connectionId: string, folderId: string | null, before: string | null) =>
    invoke<Project[]>("arrange_connection", { projectId, connectionId, folderId, before }),
  /** Moves a folder into `parent` (`null`: the root), just before the folder `before` or
   *  after the last one there. */
  arrangeConnectionFolder: (projectId: string, folderId: string, parent: string | null, before: string | null) =>
    invoke<Project[]>("arrange_connection_folder", { projectId, folderId, parent, before }),
  /** Drops the connection `dragged` onto `target`: a new folder named `name` holds both. */
  groupConnections: (projectId: string, target: string, dragged: string, name: string) =>
    invoke<{ folder: string; projects: Project[] }>("group_connections", { projectId, target, dragged, name }),
  /** Moves a connection in the list All connections shows: before `before`, or to the end. */
  reorderConnection: (connectionId: string, before: string | null) =>
    invoke<Workspace>("reorder_connection", { connectionId, before }),
  listProjectFolders: () => invoke<ProjectFolder[]>("list_project_folders"),
  /** Drops `dragged` onto `target` in the rail: both end up in one folder. */
  groupProjects: (target: string, dragged: string) => invoke<Workspace>("group_projects", { target, dragged }),
  saveProjectFolder: (folder: ProjectFolder) => invoke<Workspace>("save_project_folder", { folder }),
  /** Moves a project into a rail folder at `index`, or out of any with `null`. */
  setProjectFolder: (projectId: string, folderId: string | null, index: number | null = null) =>
    invoke<Workspace>("set_project_folder", { projectId, folderId, index }),
  ungroupProjects: (folderId: string) => invoke<Workspace>("ungroup_projects", { folderId }),
  reorderProjects: (order: string[]) => invoke<Workspace>("reorder_projects", { order }),
  deleteConnection: (id: string) => invoke<SavedConnection[]>("delete_connection", { id }),
  /** Sets a table's virtual key; no columns remove it. */
  setVirtualKey: (connectionId: string, schema: string, table: string, columns: string[]) =>
    invoke<SavedConnection[]>("set_virtual_key", { connectionId, schema, table, columns }),
  testConnection: (connection: SavedConnection, password: PasswordInput, sshSecrets: SshSecrets = {}) =>
    invoke<ServerInfo>("test_connection", { connection, password, sshSecrets }),
  /** `database` opens another database of the same server instead of the saved one. A `side`
   *  session (the navigator's) stays in autocommit mode on production too. */
  openSession: (connectionId: string, password?: string, database?: string | null, side = false) =>
    /** `minimum`: the oldest version tested, when the server is older. */
    invoke<{ session_id: number; server: ServerInfo; capabilities: Capabilities; minimum: string | null }>("open_session", {
      connectionId,
      password: password ?? null,
      database: database ?? null,
      side,
    }),
  closeSession: (sessionId: number) => invoke<void>("close_session", { sessionId }),
  /** Checks an idle session, reopening it if the server dropped it. */
  pingSession: (sessionId: number) =>
    invoke<{ state: "ok"; latency_ms: number; reconnected: boolean } | { state: "busy" }>("ping_session", {
      sessionId,
    }),
  vaultStatus: () => invoke<VaultStatus>("vault_status"),
  unlockVault: (password: string) => invoke<VaultStatus>("unlock_vault", { password }),
  lockVault: () => invoke<VaultStatus>("lock_vault"),
  /** Moves the saved passwords from the keychain into a file encrypted with `password`. */
  enableVault: (password: string) => invoke<VaultStatus>("enable_vault", { password }),
  /** Moves them back to the keychain; `password` is the current master password. */
  disableVault: (password: string) => invoke<VaultStatus>("disable_vault", { password }),
  changeVaultPassword: (current: string, password: string) =>
    invoke<VaultStatus>("change_vault_password", { current, password }),
  listNetworkProfiles: () => invoke<NetworkProfile[]>("list_network_profiles"),
  saveNetworkProfile: (profile: NetworkProfile, sshSecrets: SshSecrets = {}) =>
    invoke<NetworkProfile[]>("save_network_profile", { profile, sshSecrets }),
  /** Refused while a connection uses the profile. */
  deleteNetworkProfile: (id: string) => invoke<NetworkProfile[]>("delete_network_profile", { id }),
  /** `limit` caps fetched rows and frees the session afterwards (script mode); `history`
   *  false keeps the run out of the query history. */
  runQuery: (
    sessionId: number,
    sql: string,
    limit: number | null,
    onEvent: (e: QueryEvent) => void,
    history = true,
  ) => {
    const channel = new Channel<QueryEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("run_query", { sessionId, sql, limit, history, onEvent: channel });
  },
  listSchemas: (sessionId: number) => invoke<SchemaMeta[]>("list_schemas", { sessionId }),
  listObjects: (sessionId: number, schema: string) =>
    invoke<ObjectMeta[]>("list_objects", { sessionId, schema }),
  searchMetadata: (sessionId: number, needle: string) =>
    invoke<MetadataMatch[]>("search_metadata", { sessionId, needle }),
  searchTableData: (sessionId: number, schema: string, table: string, needle: string) =>
    invoke<TableHits>("search_table_data", { sessionId, schema, table, needle }),
  tableDetails: (sessionId: number, schema: string, table: string) =>
    invoke<TableDetails>("table_details", { sessionId, schema, table }),
  /** The columns of a table the database fills by itself; empty when unknown. */
  generatedColumns: (sessionId: number, schema: string, table: string) =>
    invoke<GeneratedColumn[]>("generated_columns", { sessionId, schema, table }),
  referencingKeys: (sessionId: number, schema: string, table: string) =>
    invoke<ReferencingKey[]>("referencing_keys", { sessionId, schema, table }),
  transactionState: (sessionId: number) => invoke<TransactionState>("transaction_state", { sessionId }),
  setAutocommit: (sessionId: number, on: boolean) =>
    invoke<TransactionState>("set_autocommit", { sessionId, on }),
  /** Commits (`commit`) or rolls back; unfinished results of the session stop fetching. */
  endTransaction: (sessionId: number, commit: boolean) =>
    invoke<TransactionState>("end_transaction", { sessionId, commit }),
  completionCatalog: (sessionId: number) => invoke<CompletionCatalog>("completion_catalog", { sessionId }),
  queryValue: (sessionId: number, sql: string) => invoke<string | null>("query_value", { sessionId, sql }),
  /** About how many rows a table has, from the statistics; `null` when unknown. */
  estimateRows: (sessionId: number, schema: string | null, table: string) =>
    invoke<number | null>("estimate_rows", { sessionId, schema, table }),
  /** `[key, label]` pairs of `values`, or with `null` the first `limit` rows by label. */
  lookupValues: (sessionId: number, lookup: Lookup, values: string[] | null, limit: number) =>
    invoke<[string | null, string | null][]>("lookup_values", { sessionId, lookup, values, limit }),
  /** Re-runs `sql` on its own connection, streaming every row to `path`. */
  exportQuery: (
    sessionId: number,
    sql: string,
    path: string,
    options: ExportOptions,
    onEvent: (e: ExportEvent) => void,
  ) => {
    const channel = new Channel<ExportEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("export_query", { sessionId, sql, path, options, onEvent: channel });
  },
  /** Writes the rows a result has already loaded. */
  exportRows: (jobId: number, path: string, options: ExportOptions) =>
    invoke<number>("export_rows", { jobId, path, options }),
  cancelExport: (exportId: number) => invoke<void>("cancel_export", { exportId }),
  /** Exports each table to `<dir>/<table>.<extension>` on its own connection; cancelled with
   *  `cancelExport`. */
  exportTables: (
    sessionId: number,
    tables: { schema: string | null; name: string }[],
    dir: string,
    options: ExportOptions,
    onEvent: (e: ExportEvent) => void,
  ) => {
    const channel = new Channel<ExportEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("export_tables", { sessionId, tables, dir, options, onEvent: channel });
  },
  /** Runs the SQL file at `path` (`.gz`, `.zst` too) on a connection of its own, with
   *  `transaction` all in one; progress counts statements. Cancelled with `cancelExport`. */
  runSqlFile: (sessionId: number, path: string, transaction: boolean, onEvent: (e: ExportEvent) => void) => {
    const channel = new Channel<ExportEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("run_sql_file", { sessionId, path, transaction, onEvent: channel });
  },
  /** Writes `schema`'s DDL, with `data` its rows too, to an SQL file at `path` (gzip or
   *  zstd by its extension); with `tables`, those objects alone. Cancelled with `cancelExport`. */
  dumpSql: (sessionId: number, schema: string, path: string, data: boolean, tables: string[] | null, onEvent: (e: ExportEvent) => void) => {
    const channel = new Channel<ExportEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("dump_sql", { sessionId, schema, path, data, tables, onEvent: channel });
  },
  /** Reads the rows `sql` (an UPDATE or DELETE) is about to change on the session, and keeps
   *  them as the statements that put them back. */
  takeSnapshot: (sessionId: number, database: string | null, sql: string) =>
    invoke<SnapshotOutcome>("take_snapshot", { sessionId, database, sql }),
  /** The snapshots kept, newest first. */
  listSnapshots: () => invoke<SnapshotMeta[]>("list_snapshots"),
  snapshotRestore: (id: string) => invoke<string[]>("snapshot_restore", { id }),
  deleteSnapshot: (id: string) => invoke<void>("delete_snapshot", { id }),
  /** The tables of a data file and the first rows of `table`, with the column types they
   *  would take in the session's database. */
  previewImport: (sessionId: number, path: string, table: string | null, options: FileOptions | null = null) =>
    invoke<ImportPreview>("preview_import", { sessionId, path, table, options }),
  /** Creates tables of the source session's schema in the target session's and copies their
   *  rows, then their foreign keys; cancelled with `cancelExport`. */
  migrateTables: (
    sourceSessionId: number,
    targetSessionId: number,
    sourceSchema: string,
    targetSchema: string,
    tables: string[],
    /** Also the schema's views, routines and triggers. */
    objects: boolean,
    onEvent: (e: ExportEvent) => void,
  ) => {
    const channel = new Channel<ExportEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("migrate_tables", {
      sourceSessionId,
      targetSessionId,
      sourceSchema,
      targetSchema,
      tables,
      objects,
      onEvent: channel,
    });
  },
  /** Adds `rows` fake rows to a table, each column filled by its generator; cancelled with
   *  `cancelExport`. */
  fakeRows: (
    sessionId: number,
    schema: string | null,
    table: string,
    rows: number,
    columns: { name: string; generator: object }[],
    onEvent: (e: ExportEvent) => void,
  ) => {
    const channel = new Channel<ExportEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("fake_rows", { sessionId, schema, table, rows, columns, onEvent: channel });
  },
  /** Saves the clipboard's text to a new `pasted.csv`; returns its path. */
  clipboardToFile: () => invoke<string>("clipboard_to_file"),
  /** Deletes a file of `clipboardToFile`. */
  discardPastedFile: (path: string) => invoke<void>("discard_pasted_file", { path }),
  /** The first rows of a table of the source session, with the column types they would take in
   *  the target session's database. */
  previewTransfer: (sourceSessionId: number, schema: string | null, table: string, targetSessionId: number) =>
    invoke<ImportPreview>("preview_transfer", { sourceSessionId, schema, table, targetSessionId }),
  /** Copies a table between the databases of two sessions; cancelled with `cancelExport`. */
  transferTable: (sourceSessionId: number, targetSessionId: number, plan: ImportPlan, onEvent: (e: ExportEvent) => void) => {
    const channel = new Channel<ExportEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("transfer_table", { sourceSessionId, targetSessionId, plan, onEvent: channel });
  },
  /** What is installed for `driver` and what the registry offers. */
  driverStatus: (driver: DriverKind) => invoke<DriverStatus>("driver_status", { driver }),
  /** Downloads and installs the driver of `driver`; stopped by `cancelDriverInstall`. */
  installDriver: (driver: DriverKind, onEvent: (e: DriverInstallEvent) => void) => {
    const channel = new Channel<DriverInstallEvent>();
    channel.onmessage = onEvent;
    return invoke<InstalledDriver>("install_driver", { driver, onEvent: channel });
  },
  /** Stops the download of `driver`'s driver. */
  cancelDriverInstall: (driver: DriverKind) => invoke<void>("cancel_driver_install", { driver }),
  listDrivers: () => invoke<InstalledDriver[]>("list_drivers"),
  /** Removes a downloaded driver; returns the ones left. */
  removeDriver: (name: string) => invoke<InstalledDriver[]>("remove_driver", { name }),
  vendorDriverStatus: (driver: DriverKind) => invoke<VendorStatus>("vendor_driver_status", { driver }),
  /** Downloads the vendor's client of `driver` and registers it where needed (UAC on Windows). */
  installVendorDriver: (driver: DriverKind, onEvent: (e: VendorInstallEvent) => void) => {
    const channel = new Channel<VendorInstallEvent>();
    channel.onmessage = onEvent;
    return invoke<VendorStatus>("install_vendor_driver", { driver, onEvent: channel });
  },
  /** Stops the download of `driver`'s client. */
  cancelVendorInstall: (driver: DriverKind) => invoke<void>("cancel_vendor_install", { driver }),
  /** Unregisters (as administrator) and deletes the vendor's client of `driver`. */
  removeVendorDriver: (driver: DriverKind) => invoke<VendorStatus>("remove_vendor_driver", { driver }),
  odbcSystem: () => invoke<OdbcSystem>("odbc_system"),
  /** Imports a data file into the session's database on its own connection; cancelled with
   *  `cancelExport`. */
  importFile: (sessionId: number, path: string, plan: ImportPlan, options: FileOptions | null, onEvent: (e: ExportEvent) => void) => {
    const channel = new Channel<ExportEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("import_file", { sessionId, path, plan, options, onEvent: channel });
  },
  analyzeSql: (sql: string, driver: DriverKind, cursor: number) =>
    invoke<SqlAnalysis>("analyze_sql", { sql, driver, cursor }),
  formatSql: (sql: string, driver: DriverKind, settings: FormatSettings) =>
    invoke<string>("format_sql", { sql, driver, settings }),
  /** The table a query reads, when its rows are that table's rows. */
  querySource: (sql: string, driver: DriverKind) => invoke<QuerySource | null>("query_source", { sql, driver }),
  lintSql: (sql: string, driver: DriverKind) => invoke<SqlLint>("lint_sql", { sql, driver }),
  /** `sql` sorted by `column` or, with `null`, back to `original` (the ORDER BY it had before
   *  the viewer's sort, from `sqlOrderClause`) or unsorted: only its top-level ORDER BY changes.
   *  `null` when it is not a read-only query the sort can be placed in with certainty. */
  sqlOrderBy: (sql: string, driver: DriverKind, column: string | null, desc: boolean, original: string | null = null) =>
    invoke<string | null>("sql_order_by", { sql, driver, column, desc, original }),
  /** The top-level ORDER BY of `sql` as written; `null` when it has none. */
  sqlOrderClause: (sql: string, driver: DriverKind) => invoke<string | null>("sql_order_clause", { sql, driver }),
  /** Whether a header click may rewrite `sql` (see `sqlOrderBy`). */
  sqlSortable: (sql: string, driver: DriverKind) => invoke<boolean>("sql_sortable", { sql, driver }),
  /** The query counting the rows of `sql`; `null` when it cannot be counted. */
  sqlCountQuery: (sql: string, driver: DriverKind) => invoke<string | null>("sql_count_query", { sql, driver }),
  sqlAddLimit: (sql: string, driver: DriverKind, limit: number) =>
    invoke<string | null>("sql_add_limit", { sql, driver, limit }),
  /** `sql` with `condition` added to its top-level WHERE; `null` when it cannot be placed. */
  sqlAddCondition: (sql: string, driver: DriverKind, condition: string) =>
    invoke<string | null>("sql_add_condition", { sql, driver, condition }),
  /** For each result column of `sql`, what a quick filter on it names in the WHERE (`name` for
   *  `name AS n`), or why there is none; `null` when no condition can be added to `sql`. */
  sqlFilterColumns: (sql: string, driver: DriverKind, columns: string[]) =>
    invoke<FilterTarget[] | null>("sql_filter_columns", { sql, driver, columns }),
  /** Saved scripts of a project, `null` for All connections. */
  listScripts: (project: string | null) => invoke<ScriptList>("list_scripts", { project }),
  /** A new empty script; returns its relative path. */
  createScript: (project: string | null, folder: string, name: string, connection: string | null) =>
    invoke<string>("create_script", { project, folder, name, connection }),
  createScriptFolder: (project: string | null, path: string) => invoke<void>("create_script_folder", { project, path }),
  renameScript: (project: string | null, from: string, to: string) => invoke<void>("rename_script", { project, from, to }),
  deleteScript: (project: string | null, path: string) => invoke<void>("delete_script", { project, path }),
  linkScript: (project: string | null, path: string, connection: string | null) =>
    invoke<void>("link_script", { project, path, connection }),
  /** Runs a script with `psql`, `mysql`, `sqlcmd` or `sqlite3` from the PATH, as the session;
   *  `cancelCli` with the same `runId` stops it. */
  runCli: (sessionId: number, runId: number, script: string) =>
    invoke<CliOutput>("run_cli", { sessionId, runId, script }),
  cancelCli: (runId: number) => invoke<void>("cancel_cli", { runId }),
  /** Backs the session's database up with `pg_dump` or `mysqldump`; `cancelCli(runId)` stops it. */
  backupDatabase: (sessionId: number, runId: number, request: BackupRequest, onEvent: (e: BackupEvent) => void) => {
    const channel = new Channel<BackupEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("backup_database", { sessionId, runId, request, onEvent: channel });
  },
  /** Restores a backup into the session's database; `cancelCli(runId)` stops it. */
  restoreDatabase: (sessionId: number, runId: number, request: RestoreRequest, onEvent: (e: BackupEvent) => void) => {
    const channel = new Channel<BackupEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("restore_database", { sessionId, runId, request, onEvent: channel });
  },
  listTransferProfiles: () => invoke<TransferProfile[]>("list_transfer_profiles"),
  /** Saves a profile, new or changed (by id); returns it without the secret key of its bucket. */
  saveTransferProfile: (profile: TransferProfile) => invoke<TransferProfile>("save_transfer_profile", { profile }),
  /** Refused while a scheduled task runs the profile; returns the profiles left. */
  deleteTransferProfile: (id: string) => invoke<TransferProfile[]>("delete_transfer_profile", { id }),
  /** Runs a profile as its dialog would; cancelled with `cancelExport`. */
  runTransferProfile: (id: string, onEvent: (e: ExportEvent) => void) => {
    const channel = new Channel<ExportEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("run_transfer_profile", { id, onEvent: channel });
  },
  listSchedules: () => invoke<ScheduleView[]>("list_schedules"),
  /** Saves a task, new or changed; returns them all. */
  saveSchedule: (schedule: Schedule) => invoke<ScheduleView[]>("save_schedule", { schedule }),
  deleteSchedule: (id: string) => invoke<ScheduleView[]>("delete_schedule", { id }),
  /** Runs a task now; the list hears when it starts and ends (`schedules-changed`). */
  runScheduleNow: (id: string) => invoke<void>("run_schedule_now", { id }),
  timeZones: () => invoke<string[]>("time_zones"),
  /** `connectionIds` limits the entries to those connections, e.g. a project's. */
  searchHistory: (query: {
    connectionIds?: string[];
    text?: string;
    beforeId?: number;
    /** Only the app's entries, or only the agents'; both when left out. */
    origin?: HistoryOrigin | null;
    limit: number;
  }) =>
    invoke<HistoryEntry[]>("search_history", {
      connectionIds: query.connectionIds ?? null,
      text: query.text || null,
      beforeId: query.beforeId ?? null,
      origin: query.origin ?? null,
      limit: query.limit,
    }),
  deleteHistory: (id: number) => invoke<void>("delete_history", { id }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  writeTextFile: (path: string, contents: string) =>
    invoke<void>("write_text_file", { path, contents }),
  readClipboard: () => invoke<string>("read_clipboard"),
  /** Rows as column-major text arrays (`null` = SQL NULL). */
  fetchRows: async (jobId: number, offset: number, limit: number) => {
    const bytes = await invoke<ArrayBuffer>("fetch_rows", { jobId, offset, limit });
    const table = tableFromIPC(new Uint8Array(bytes));
    const columns: (string | null)[][] = [];
    for (let c = 0; c < table.numCols; c++) {
      columns.push(Array.from(table.getChildAt(c) ?? [], (v) => (v == null ? null : String(v))));
    }
    return { rows: table.numRows, columns };
  },
  /** Stored preferences as nested objects; see `settings.ts`. */
  getSettings: () => invoke<Record<string, unknown>>("get_settings"),
  /** `null` removes the key, restoring the default. */
  setSetting: (key: string, value: unknown) => invoke<void>("set_setting", { key, value }),
  /** The query log's entries that changed after revision `since` (0: all). */
  queryLog: (since: number) => invoke<{ entries: LogEntry[]; rev: number }>("query_log", { since }),
  clearQueryLog: () => invoke<void>("clear_query_log"),
  /** With `analyze` the statement runs, and its changes are rolled back. */
  explain: (sessionId: number, sql: string, analyze: boolean) => invoke<Explain>("explain", { sessionId, sql, analyze }),
  /** Reads a plan pasted or opened from a file; `source` `null` guesses whose it is. */
  readPlan: (document: string, source: PlanSource | null) => invoke<ReadPlan>("read_plan", { document, source }),
  serverSessions: (sessionId: number) => invoke<ServerSession[]>("server_sessions", { sessionId }),
  serverLocks: (sessionId: number) => invoke<ServerLock[]>("server_locks", { sessionId }),
  topQueries: (sessionId: number) => invoke<ServerQuery[]>("top_queries", { sessionId }),
  objectStatistics: (sessionId: number) => invoke<ObjectStatistics[]>("object_statistics", { sessionId }),
  /** Runs `operation` on table `schema.table`; returns what the server said, a line each. */
  maintainTable: (sessionId: number, schema: string, table: string, operation: Maintenance) =>
    invoke<string[]>("maintain_table", { sessionId, schema, table, operation }),
  endServerSession: (sessionId: number, id: string, how: EndSession) =>
    invoke<void>("end_server_session", { sessionId, id, how }),
  objectDdl: (sessionId: number, object: ObjectTarget) => invoke<string>("object_ddl", { sessionId, object }),
  objectProperties: (sessionId: number, object: ObjectTarget) =>
    invoke<Property[]>("object_properties", { sessionId, object }),
  listDatabases: (sessionId: number) => invoke<string[]>("list_databases", { sessionId }),
  /** Runs DDL or DML whose result is not shown; the affected rows when reported. */
  executeStatement: (sessionId: number, sql: string) => invoke<number | null>("execute_statement", { sessionId, sql }),
  /** `null` when the dialect has no such statement for the object. */
  generateSql: (driver: DriverKind, target: GenerateTarget, request: GenerateRequest) =>
    invoke<string | null>("generate_sql", { driver, target, request }),
  dialectSupport: (driver: DriverKind) => invoke<DialectSupport>("dialect_support", { driver }),
  tableDesign: (sessionId: number, schema: string, table: string) =>
    invoke<TableDesign>("table_design", { sessionId, schema, table }),
  /** The script from `original` to `design`, or creating `design` when `original` is null. */
  tableScript: (driver: DriverKind, schema: string, original: TableDesign | null, design: TableDesign) =>
    invoke<{ script: TableScript; sql: string }>("table_script", { driver, schema, original, design }),
  applyTableScript: (sessionId: number, script: TableScript) =>
    invoke<void>("apply_table_script", { sessionId, script }),
  /** The tables of `schema` with their details: those in `tables`, or all when null. */
  schemaTables: (sessionId: number, schema: string, tables: string[] | null) =>
    invoke<SchemaTable[]>("schema_tables", { sessionId, schema, tables }),
  lintSchema: (sessionId: number, schema: string) => invoke<LintFinding[]>("lint_schema", { sessionId, schema }),
  /** The script that makes the target schema like the source one. */
  compareSchemas: (sourceSession: number, sourceSchema: string, targetSession: number, targetSchema: string) =>
    invoke<{ script: TableScript; sql: string }>("compare_schemas", { sourceSession, sourceSchema, targetSession, targetSchema }),
  /** The UPDATE, INSERT and DELETE statements that saving `changes` would run. */
  previewChanges: (jobId: number, changes: TableChanges) =>
    invoke<string[]>("preview_changes", { jobId, changes }),
  /** Saves `changes` on the result's session, all or none; `undo` puts the rows back. */
  saveChanges: (jobId: number, changes: TableChanges) =>
    invoke<{ statements: number; undo: string }>("save_changes", { jobId, changes }),
  /** Loaded rows' values in full, not cut to one line like the grid's. With `columns`, only
   *  those columns of each row, in that order. */
  rowValues: (jobId: number, offset: number, limit: number, columns?: readonly number[]) =>
    invoke<(string | null)[][]>("row_values", { jobId, offset, limit, columns: columns ?? null }),
  /** The elements of a PostgreSQL array (or the fields of a composite value); `null` when
   *  the text is not one, e.g. a nested array. */
  /** `typeName` is the array's type (`box[]` separates its elements with `;`). */
  splitLiteral: (composite: boolean, typeName: string, text: string) =>
    invoke<(string | null)[] | null>("split_literal", { composite, typeName, text }),
  joinLiteral: (composite: boolean, typeName: string, items: (string | null)[]) =>
    invoke<string>("join_literal", { composite, typeName, items }),
  /** Loaded rows of a result in a clipboard format; `table` names the table in INSERTs. */
  copyRows: (
    jobId: number,
    offset: number,
    limit: number,
    columns: number[],
    format: CopyFormat,
    options: { header?: boolean; table?: string | null } = {},
  ) =>
    invoke<string>("copy_rows", {
      jobId,
      offset,
      limit,
      columns,
      format,
      header: options.header ?? false,
      table: options.table ?? null,
    }),
  /** A column's loaded values as SQL literals, `NULL` included. */
  sqlLiterals: (jobId: number, offset: number, limit: number, column: number) =>
    invoke<string[]>("sql_literals", { jobId, offset, limit, column }),
  /** A cell's raw bytes (UTF-8 text for non-binary columns); empty for NULL. */
  cellBytes: async (jobId: number, row: number, column: number) =>
    new Uint8Array(await invoke<ArrayBuffer>("cell_bytes", { jobId, row, column })),
  saveCell: (jobId: number, row: number, column: number, path: string) =>
    invoke<void>("save_cell", { jobId, row, column, path }),
  /** A file's size in bytes. */
  fileSize: (path: string) => invoke<number>("file_size", { path }),
  /** A file's contents for a data editor cell: hex for a binary column, else UTF-8 text
   *  (an `invalid_utf8` error otherwise). */
  readCellFile: (path: string, binary: boolean) => invoke<string>("read_cell_file", { path, binary }),
  /** Loaded cells containing `text`, ignoring case. */
  selectionStats: (jobId: number, offset: number, limit: number, columns: number[]) =>
    invoke<SelectionStats>("selection_stats", { jobId, offset, limit, columns }),
  /** Statistics of each of `columns` over loaded rows `offset..offset + limit`. */
  columnStats: (jobId: number, offset: number, limit: number, columns: number[]) =>
    invoke<ColumnStats[]>("column_stats", { jobId, offset, limit, columns }),
  /** The geometries in `columns` of loaded rows `offset..offset + limit`. */
  geometries: (jobId: number, offset: number, limit: number, columns: number[]) =>
    invoke<Geometries>("geometries", { jobId, offset, limit, columns }),
  /** Loaded rows by value of `columns`, most frequent first. */
  groupCounts: (jobId: number, columns: number[]) => invoke<GroupCounts>("group_counts", { jobId, columns }),
  searchRows: (jobId: number, text: string) => invoke<SearchResult>("search_rows", { jobId, text }),
  fetchAll: (jobId: number) => invoke<void>("fetch_all", { jobId }),
  cancelQuery: (jobId: number) => invoke<void>("cancel_query", { jobId }),
  closeQuery: (jobId: number) => invoke<void>("close_query", { jobId }),
  catalogEntries: (connectionId: string) => invoke<CatalogEntry[]>("catalog_entries", { connectionId }),
  saveCatalogEntry: (connectionId: string, key: CatalogKey, note: string, tags: string[]) =>
    invoke<CatalogEntry>("save_catalog_entry", { connectionId, key, note, tags }),
  catalogTags: (connectionId: string) => invoke<TagCount[]>("catalog_tags", { connectionId }),
  renameCatalogKey: (connectionId: string, from: CatalogKey, to: CatalogKey) =>
    invoke<void>("rename_catalog_key", { connectionId, from, to }),
  commentSql: (
    sessionId: number,
    schema: string,
    table: string,
    column: string | null,
    comment: string | null,
) => invoke<string>("comment_sql", { sessionId, schema, table, column, comment }),
  catalogDisplayColumns: (connectionId: string) =>
    invoke<DisplayColumn[]>("catalog_display_columns", { connectionId }),
  setCatalogDisplayColumn: (connectionId: string, key: CatalogKey, column: string | null) =>
    invoke<void>("set_catalog_display_column", { connectionId, key, column }),
  /** Writes the documentation of `request.schemas`; cancelled with `cancelDocs`. */
  exportDocs: (
    sessionId: number,
    connectionId: string,
    database: string | null,
    request: DocsRequest,
    onEvent: (e: DocsEvent) => void,
  ) => {
    const channel = new Channel<DocsEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("export_docs", { sessionId, connectionId, database, request, onEvent: channel });
  },
  cancelDocs: (exportId: number) => invoke<void>("cancel_docs", { exportId }),

  // Analysis (group M): column statistics
  /** The profile of `column` over the rows the result has loaded. */
  columnProfile: (jobId: number, column: number) => invoke<ColumnProfile>("column_profile", { jobId, column }),

  // Analysis (group M): charts
  /** What `request` draws from the rows the result has loaded. */
  chartData: (jobId: number, request: ChartRequest) => invoke<ChartData>("chart_data", { jobId, request }),
  /** Writes bytes to a path picked in the save dialog, e.g. a chart as PNG. */
  writeBinaryFile: (path: string, bytes: Uint8Array) =>
    invoke<void>("write_binary_file", { path, bytes: Array.from(bytes) }),

  // Analysis (group M): SQL on results
  /** Loads every row of a result up to the memory limit; `onProgress` gets the rows loaded.
   *  True when they are all of its rows. Fails when the result is closed or reading fails. */
  fetchWhole: (jobId: number, onProgress: (rows: number) => void) => {
    const channel = new Channel<number>();
    channel.onmessage = onProgress;
    return invoke<boolean>("fetch_whole", { jobId, onProgress: channel });
  },
  /** Stops loading a result, keeping its rows; nothing for a closed result. */
  stopFetch: (jobId: number) => invoke<void>("stop_fetch", { jobId }),
  /** Copies the results into a new in-memory DuckDB; returns its temporary connection. */
  openResultDb: (name: string, tables: ResultTable[]) => invoke<OpenedResultDb>("open_result_db", { name, tables }),
  /** Copies the results again into the temporary connection's tables; why one lacks rows. */
  reloadResultDb: (connectionId: string, tables: ResultTable[]) =>
    invoke<ResultPartial | null>("reload_result_db", { connectionId, tables }),
  closeResultDb: (connectionId: string) => invoke<void>("close_result_db", { connectionId }),

  // Analysis (group M): compare with the previous run
  /** Keeps the loaded rows of the result a run of the same statement replaces. */
  keepPreviousRun: (sessionId: number, jobId: number) => invoke<void>("keep_previous_run", { sessionId, jobId }),
  forgetPreviousRun: (sessionId: number) => invoke<void>("forget_previous_run", { sessionId }),
  /** The differences from the kept run to result `jobId`, by the `key` columns when given. */
  diffPrevious: (sessionId: number, jobId: number, key: number[] | null) =>
    invoke<Diff>("diff_previous", { sessionId, jobId, key }),
};
