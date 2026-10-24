import { Channel, invoke } from "@tauri-apps/api/core";
import { tableFromIPC } from "apache-arrow";

export type DriverKind = "postgres" | "mysql" | "sqlite" | "duckdb";

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
  color: string | null;
  /** Production asks before changes and starts in manual commit. */
  connection_type?: ConnectionType | null;
  /** The SSH server the connection goes through. */
  ssh?: SavedSsh | null;
  /** Logical keys set by the user for tables without a primary key. */
  virtual_keys?: VirtualKey[];
}

export type ConnectionType = "dev" | "test" | "prod";

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
}

export interface CommandError {
  kind: string;
  message: string;
  code: string | null;
  detail: string | null;
  hint: string | null;
  position: number | null;
}

export interface ColumnInfo {
  name: string;
  db_type: string;
  numeric: boolean;
  /** Bytes, shown as hex. */
  binary: boolean;
}

export type QueryEvent =
  | { type: "started"; columns: ColumnInfo[]; elapsed_ms: number }
  | { type: "progress"; rows: number }
  | { type: "finished"; rows: number; elapsed_ms: number; complete: boolean; stopped: boolean }
  | { type: "affected"; rows: number; elapsed_ms: number }
  | { type: "failed"; error: CommandError };

/** Offsets are UTF-16 code units, like JavaScript string indices. */
export interface StatementSpan {
  start: number;
  end: number;
  read_only: boolean;
  /** `command` is a client command such as `@set x = 1`, run by the app. */
  kind: "query" | "dml" | "ddl" | "other" | "command";
  /** An UPDATE or DELETE without WHERE, or a TRUNCATE: the table it changes in every row. */
  unfiltered: { schema: string | null; name: string } | null;
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
}

export interface HistoryEntry {
  id: number;
  connection_id: string;
  sql: string;
  /** Unix epoch milliseconds. */
  started_at: number;
  duration_ms: number;
  rows: number | null;
  status: "ok" | "error" | "cancelled";
  error: string | null;
}

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
  | "event";

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

/** Pending changes to a result of one table; see `commands/data.rs`. */
export interface TableChanges {
  table: { schema: string; name: string };
  /** Result columns that identify a row. */
  key: number[];
  /** Result columns that are the table's own, not expressions. */
  table_columns: number[];
  /** Default expressions by result column. */
  defaults: (string | null)[];
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

export type ExportFormat = "csv" | "json" | "ndjson" | "parquet";

export interface ExportOptions {
  format: ExportFormat;
  delimiter: string;
  header: boolean;
}

export type ExportEvent =
  | { type: "progress"; rows: number }
  | { type: "done"; rows: number; elapsed_ms: number }
  | { type: "failed"; error: CommandError }
  | { type: "cancelled" };

/** A set of connections worked on together; a connection can be in several. */
export interface Project {
  id: string;
  name: string;
  /** `#rrggbb`. */
  color: string;
  /** One or two characters on the project's square. */
  initials: string;
  connections: string[];
}

export interface AppInfo {
  name: string;
  version: string;
  /** Developer tools are open, as for the end-to-end tests: the page gets test hooks. */
  devtools: boolean;
  os: string;
  arch: string;
}

/** Normalizes anything thrown by `invoke` into a `CommandError`. */
export function toCommandError(e: unknown): CommandError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) {
    return e as CommandError;
  }
  return { kind: "other", message: String(e), code: null, detail: null, hint: null, position: null };
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  listConnections: () => invoke<SavedConnection[]>("list_connections"),
  defaultConnection: (driver: DriverKind) =>
    invoke<SavedConnection>("default_connection", { driver }),
  /** `projects` replaces the projects the connection is in; `null` leaves them. */
  saveConnection: (
    connection: SavedConnection,
    password: PasswordInput,
    sshSecrets: SshSecrets = {},
    projects: string[] | null = null,
  ) => invoke<SavedConnection[]>("save_connection", { connection, password, sshSecrets, projects }),
  detectImports: () => invoke<ImportFile[]>("detect_imports"),
  /** The connections in another tool's file; nothing is saved yet. */
  readImport: (source: ImportSource, path: string) => invoke<SavedConnection[]>("read_import", { source, path }),
  /** Saves imported connections, into `project` when given. */
  saveImported: (connections: SavedConnection[], project: string | null = null) =>
    invoke<SavedConnection[]>("save_imported", { connections, project }),
  listProjects: () => invoke<Project[]>("list_projects"),
  saveProject: (project: Project) => invoke<Project[]>("save_project", { project }),
  /** Deletes a project; its connections stay. */
  deleteProject: (id: string) => invoke<Project[]>("delete_project", { id }),
  setProjectMembership: (connectionId: string, projectId: string, member: boolean) =>
    invoke<Project[]>("set_project_membership", { connectionId, projectId, member }),
  /** The initials a project gets from its name. */
  projectInitials: (name: string) => invoke<string>("project_initials", { name }),
  deleteConnection: (id: string) => invoke<SavedConnection[]>("delete_connection", { id }),
  /** Sets a table's virtual key; no columns remove it. */
  setVirtualKey: (connectionId: string, schema: string, table: string, columns: string[]) =>
    invoke<SavedConnection[]>("set_virtual_key", { connectionId, schema, table, columns }),
  testConnection: (connection: SavedConnection, password: PasswordInput, sshSecrets: SshSecrets = {}) =>
    invoke<ServerInfo>("test_connection", { connection, password, sshSecrets }),
  /** `database` opens another database of the same server instead of the saved one. */
  openSession: (connectionId: string, password?: string, database?: string | null) =>
    invoke<{ session_id: number; server: ServerInfo; capabilities: Capabilities }>("open_session", {
      connectionId,
      password: password ?? null,
      database: database ?? null,
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
  analyzeSql: (sql: string, driver: DriverKind, cursor: number) =>
    invoke<SqlAnalysis>("analyze_sql", { sql, driver, cursor }),
  formatSql: (sql: string, driver: DriverKind, settings: FormatSettings) =>
    invoke<string>("format_sql", { sql, driver, settings }),
  /** The table a query reads, when its rows are that table's rows. */
  querySource: (sql: string, driver: DriverKind) => invoke<QuerySource | null>("query_source", { sql, driver }),
  lintSql: (sql: string, driver: DriverKind) => invoke<SqlLint>("lint_sql", { sql, driver }),
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
  /** Runs a script with `psql`, `mysql` or `sqlite3` from the PATH, as the session;
   *  `cancelCli` with the same `runId` stops it. */
  runCli: (sessionId: number, runId: number, script: string) =>
    invoke<CliOutput>("run_cli", { sessionId, runId, script }),
  cancelCli: (runId: number) => invoke<void>("cancel_cli", { runId }),
  /** `connectionIds` limits the entries to those connections, e.g. a project's. */
  searchHistory: (query: {
    connectionIds?: string[];
    text?: string;
    beforeId?: number;
    limit: number;
  }) =>
    invoke<HistoryEntry[]>("search_history", {
      connectionIds: query.connectionIds ?? null,
      text: query.text || null,
      beforeId: query.beforeId ?? null,
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
  /** The UPDATE, INSERT and DELETE statements that saving `changes` would run. */
  previewChanges: (jobId: number, changes: TableChanges) =>
    invoke<string[]>("preview_changes", { jobId, changes }),
  /** Saves `changes` on the result's session, all or none; `undo` puts the rows back. */
  saveChanges: (jobId: number, changes: TableChanges) =>
    invoke<{ statements: number; undo: string }>("save_changes", { jobId, changes }),
  /** Loaded rows' values in full, not cut to one line like the grid's. */
  rowValues: (jobId: number, offset: number, limit: number) =>
    invoke<(string | null)[][]>("row_values", { jobId, offset, limit }),
  /** The elements of a PostgreSQL array (or the fields of a composite value); `null` when
   *  the text is not one, e.g. a nested array. */
  splitLiteral: (composite: boolean, text: string) =>
    invoke<(string | null)[] | null>("split_literal", { composite, text }),
  joinLiteral: (composite: boolean, items: (string | null)[]) =>
    invoke<string>("join_literal", { composite, items }),
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
};
