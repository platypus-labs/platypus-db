/**
 * SQL on a result: the one-line field above a result's grid runs a query, or a fragment of
 * one, on a copy of the loaded rows in an in-memory DuckDB, and the grid shows its output in
 * place of the result. This is the pure part (the unit tests import it); `ResultSql.tsx` holds
 * the field and its state.
 */
import type { ColumnInfo, CompletionCatalog, QueryEvent, ResultTable } from "../api";
import type { Result } from "../components/ResultPanel";

/** Words that start a whole query, run as typed. DuckDB also reads `FROM t` alone. */
const QUERY_START = /^(?:\(|(?:select|with|from|values|table|describe|summarize|pivot|unpivot|explain|show)\b)/i;
/** Clauses that continue `SELECT … FROM result`. */
const CLAUSE_START =
  /^(?:where|group\s+by|order\s+by|having|limit|offset|qualify|window|using\s+sample|(?:natural\s+|cross\s+|positional\s+|asof\s+)?(?:(?:left|right|full|inner|semi|anti)\s+(?:outer\s+)?)?join)\b/i;
/** Clauses after `GROUP BY`'s expressions. */
const AFTER_GROUP = /\b(?:having|order\s+by|limit|offset|qualify|window)\b/i;

/** `sql` with quoted text, comments and what parentheses hold blanked out, so that a keyword
 *  found in it is one of the statement's own clauses. Same length as `sql`. */
export function topLevel(sql: string): string {
  let out = "";
  let depth = 0;
  let quote: string | null = null;
  for (let i = 0; i < sql.length; i++) {
    const c = sql[i]!;
    if (quote) {
      if (c === quote) quote = null;
      out += " ";
    } else if (c === "'" || c === '"') {
      quote = c;
      out += " ";
    } else if (c === "(") {
      depth++;
      out += " ";
    } else if (c === ")") {
      depth = Math.max(0, depth - 1);
      out += " ";
    } else {
      out += depth > 0 ? " " : c;
    }
  }
  return out;
}

/** What `SELECT` lists for `fragment`: with a `GROUP BY`, its expressions and the count of
 *  each group, since `*` would name columns that are not grouped. */
function selectList(fragment: string): string {
  const masked = topLevel(fragment);
  const group = /\bgroup\s+by\b/i.exec(masked);
  if (!group) return "*";
  const start = group.index + group[0].length;
  const end = AFTER_GROUP.exec(masked.slice(start));
  const exprs = fragment.slice(start, end ? start + end.index : fragment.length).trim();
  if (!exprs) return "*";
  return `${/^all$/i.test(exprs) ? "*" : exprs}, count(*) AS count`;
}

/** The query the field's `text` runs: a whole query as typed; a clause (`WHERE …`, `ORDER BY
 *  …`, `GROUP BY …`, a join) after `SELECT … FROM result`; anything else as the condition of
 *  a `WHERE`. `null` for no text. */
export function resultQuery(text: string): string | null {
  const sql = text.trim().replace(/;+\s*$/, "").trim();
  if (!sql) return null;
  if (QUERY_START.test(sql)) return sql;
  const fragment = CLAUSE_START.test(sql) ? sql : `WHERE ${sql}`;
  return `SELECT ${selectList(fragment)} FROM result ${fragment}`;
}

/** The copy holds other results than `wanted` (the tab ran again): it must be loaded again. */
export function staleCopy(loaded: readonly ResultTable[], wanted: readonly ResultTable[]): boolean {
  const key = (tables: readonly ResultTable[]) =>
    tables
      .map((t) => `${t.table}=${t.job_id}`)
      .sort()
      .join(",");
  return key(loaded) !== key(wanted);
}

/** The tables of the copy and their columns, for the field's completion. */
export function resultCatalog(tables: readonly { table: string; columns: readonly ColumnInfo[] }[]): CompletionCatalog {
  const main: Record<string, string[]> = {};
  for (const t of tables) main[t.table] = t.columns.map((c) => c.name);
  return { default_schema: "main", schemas: { main }, foreign_keys: [] };
}

/** `result` after one event of its query, as the SQL editor applies them. */
export function withEvent(result: Result, event: QueryEvent): Result {
  switch (event.type) {
    case "started":
      return {
        ...result,
        columns: event.columns,
        fetching: true,
        status: { kind: "rows", finished: false, complete: true, elapsedMs: event.elapsed_ms },
      };
    case "progress":
      return { ...result, rows: event.rows, fetching: event.fetching };
    case "finished":
      return {
        ...result,
        rows: event.rows,
        fetching: false,
        status: { kind: "rows", finished: true, complete: event.complete, stopped: event.stopped, elapsedMs: event.elapsed_ms },
      };
    case "affected":
      return { ...result, status: { kind: "affected", rows: event.rows, elapsedMs: event.elapsed_ms } };
    case "failed":
      return { ...result, status: { kind: "error", error: event.error } };
  }
}

/** The query has a job and has started, failed or ended: the grid can show it (or the field
 *  its error). */
export const settled = (result: Result): boolean =>
  result.jobId !== null && result.status.kind !== "running" && result.status.kind !== "pending";

/** The field's text on one line: a query from the larger editor may span several. */
export const oneLine = (text: string): string => text.replace(/\s*\n\s*/g, " ");
