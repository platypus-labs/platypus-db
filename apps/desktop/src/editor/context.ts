// Where the editor's session is after the statements it ran: plain logic of QueryTab, tested
// on its own (tests/context.test.mjs).
import type { DriverKind } from "../api";

/** The database `sql` switches the session to: `USE x` on MySQL and SQL Server, `use x` in
 *  the MongoDB shell; `null` for any other statement. */
export function usedDatabase(sql: string, driver: DriverKind): string | null {
  if (driver !== "mysql" && driver !== "sqlserver" && driver !== "mongo") return null;
  const m = /^\s*use\s+(`([^`]+)`|\[([^\]]+)\]|"([^"]+)"|([^\s;]+))\s*;?\s*$/i.exec(sql);
  return m ? (m[2] ?? m[3] ?? m[4] ?? m[5] ?? null) : null;
}

/** Whether `sql` changes the schema names resolve in: PostgreSQL's `search_path`. */
export function setsSchema(sql: string): boolean {
  return /^\s*set\s+(session\s+|local\s+)?(search_path|schema)\b/i.test(sql);
}
