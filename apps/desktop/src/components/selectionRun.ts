/**
 * How a selection in the SQL editor runs (Ctrl+Enter with text selected): as typed, in one
 * piece, or statement by statement as a script.
 */
import type { DriverKind } from "../api";

/** Drivers that run several statements sent at once and return each one's result (a T-SQL
 *  batch, MySQL's multi-statements), so that the selection keeps the batch's meaning, its
 *  variables included; an ODBC or plugin driver's server is unknown, and the NoSQL editors
 *  read their text their own way. Elsewhere PostgreSQL would show only the first result, and
 *  SQLite, ClickHouse and the others refuse several statements. */
const BATCHES = new Set<DriverKind>(["sqlserver", "mysql", "odbc", "plugin", "mongo", "redis", "elasticsearch", "cassandra"]);

/** Whether a selection holding statements of `kinds` runs one statement at a time: client
 *  commands always do, and several statements where the driver does not batch them. */
export function selectionAsScript(kinds: readonly string[], driver: DriverKind): boolean {
  return kinds.includes("command") || (kinds.length > 1 && !BATCHES.has(driver));
}
