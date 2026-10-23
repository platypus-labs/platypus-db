import { Channel, invoke } from "@tauri-apps/api/core";
import { tableFromIPC } from "apache-arrow";

export type DriverKind = "postgres" | "mysql" | "sqlite";
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
  save_password: boolean;
  read_only: boolean;
  color: string | null;
  folder: string | null;
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
  /** The table of a trigger. */
  parent: string | null;
}

export interface ColumnMeta {
  name: string;
  data_type: string;
  nullable: boolean;
  default: string | null;
  primary_key: boolean;
  comment: string | null;
}

export interface IndexMeta {
  name: string;
  columns: string[];
  unique: boolean;
  primary: boolean;
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
export interface ReferenceMeta {
  name: string;
  schema: string;
  table: string;
  columns: string[];
  ref_columns: string[];
}

export interface TableDetails {
  columns: ColumnMeta[];
  indexes: IndexMeta[];
  foreign_keys: ForeignKeyMeta[];
  constraints: ConstraintMeta[];
  referenced_by: ReferenceMeta[];
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

/** Tables and columns by schema, for autocompletion. */
export interface CompletionCatalog {
  default_schema: string | null;
  schemas: Record<string, Record<string, string[]>>;
}

export interface TransactionState {
  autocommit: boolean;
  open: boolean;
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

export interface AppInfo {
  name: string;
  version: string;
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
  saveConnection: (connection: SavedConnection, password: PasswordInput) =>
    invoke<SavedConnection[]>("save_connection", { connection, password }),
  deleteConnection: (id: string) => invoke<SavedConnection[]>("delete_connection", { id }),
  testConnection: (connection: SavedConnection, password: PasswordInput) =>
    invoke<ServerInfo>("test_connection", { connection, password }),
  /** `database` opens another database of the same server instead of the saved one. */
  openSession: (connectionId: string, password?: string, database?: string | null) =>
    invoke<{ session_id: number; server: ServerInfo; capabilities: Capabilities }>("open_session", {
      connectionId,
      password: password ?? null,
      database: database ?? null,
    }),
  closeSession: (sessionId: number) => invoke<void>("close_session", { sessionId }),
  /** `limit` caps fetched rows and frees the session afterwards (script mode). */
  runQuery: (
    sessionId: number,
    sql: string,
    limit: number | null,
    onEvent: (e: QueryEvent) => void,
  ) => {
    const channel = new Channel<QueryEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("run_query", { sessionId, sql, limit, onEvent: channel });
  },
  listSchemas: (sessionId: number) => invoke<SchemaMeta[]>("list_schemas", { sessionId }),
  listObjects: (sessionId: number, schema: string) =>
    invoke<ObjectMeta[]>("list_objects", { sessionId, schema }),
  tableDetails: (sessionId: number, schema: string, table: string) =>
    invoke<TableDetails>("table_details", { sessionId, schema, table }),
  transactionState: (sessionId: number) => invoke<TransactionState>("transaction_state", { sessionId }),
  setAutocommit: (sessionId: number, on: boolean) =>
    invoke<TransactionState>("set_autocommit", { sessionId, on }),
  /** Commits (`commit`) or rolls back; unfinished results of the session stop fetching. */
  endTransaction: (sessionId: number, commit: boolean) =>
    invoke<TransactionState>("end_transaction", { sessionId, commit }),
  completionCatalog: (sessionId: number) => invoke<CompletionCatalog>("completion_catalog", { sessionId }),
  queryValue: (sessionId: number, sql: string) => invoke<string | null>("query_value", { sessionId, sql }),
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
  searchHistory: (query: {
    connectionId?: string;
    text?: string;
    beforeId?: number;
    limit: number;
  }) =>
    invoke<HistoryEntry[]>("search_history", {
      connectionId: query.connectionId ?? null,
      text: query.text || null,
      beforeId: query.beforeId ?? null,
      limit: query.limit,
    }),
  deleteHistory: (id: number) => invoke<void>("delete_history", { id }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  writeTextFile: (path: string, contents: string) =>
    invoke<void>("write_text_file", { path, contents }),
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
  fetchAll: (jobId: number) => invoke<void>("fetch_all", { jobId }),
  cancelQuery: (jobId: number) => invoke<void>("cancel_query", { jobId }),
  closeQuery: (jobId: number) => invoke<void>("close_query", { jobId }),
};
