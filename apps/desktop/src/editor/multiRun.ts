/**
 * Running one script on several connections: the plan of each run and how the results come
 * together. The last result with rows of each connection becomes a table of an in-memory
 * DuckDB (as "Query with SQL" does), and one query puts them under each other, the
 * connection's name in a column of its own and missing columns NULL.
 */

import type { SqlAnalysis } from "../api";

/** How a connection's run ended, or where it is. */
export type Outcome =
  | { state: "queued" }
  | { state: "running"; statement: number }
  /** `partial`: the memory limit stopped reading the rows. */
  | { state: "done"; statements: number; rows: number | null; affected: number; partial: boolean }
  | { state: "failed"; statement: number; message: string }
  | { state: "skipped"; reason: string };

/** A connection's result kept for the merge: its table in the in-memory database. */
export interface Kept {
  name: string;
  table: string;
  columns: string[];
}

/** A name in DuckDB, the merged results' database, always quoted. Not `sqlIdent.ts`'s
 *  `quoteIdent`: this module imports no values, so that its tests run in Node. */
const duckIdent = (name: string) => `"${name.replaceAll('"', '""')}"`;
/** `'text'`, doubling the quotes inside. */
export const quoteText = (text: string) => `'${text.replaceAll("'", "''")}'`;

/** The name of the column that says which connection a row is from: `connection`, unless a
 *  result has a column of that name already (any case, as DuckDB compares them). */
export function connectionColumn(columns: readonly (readonly string[])[]): string {
  const taken = new Set(columns.flat().map((c) => c.toLowerCase()));
  let name = "connection";
  for (let i = 2; taken.has(name); i++) name = `connection_${i}`;
  return name;
}

/** The query that puts the kept results under each other, in the order given. */
export function mergedSql(kept: readonly Kept[]): string {
  const column = duckIdent(connectionColumn(kept.map((k) => k.columns)));
  const tables = kept.map((k) => `--   ${k.table}: ${k.name.replaceAll("\n", " ")}`).join("\n");
  const selects = kept.map((k) => `SELECT ${quoteText(k.name)} AS ${column}, * FROM ${duckIdent(k.table)}`);
  return `-- One result per connection:\n${tables}\n${selects.join("\nUNION ALL BY NAME\n")}`;
}

/** Table names for the kept results: `c1`, `c2`, … in order. */
export const tableOf = (i: number) => `c${i + 1}`;

/** Runs `work` on each item, at most `limit` at a time, in order of start; resolves once
 *  all have ended. `stopped` checked before each start skips the rest. */
export async function pool<T>(items: readonly T[], limit: number, work: (item: T, index: number) => Promise<void>, stopped: () => boolean) {
  let next = 0;
  const lane = async () => {
    while (next < items.length && !stopped()) {
      const i = next++;
      await work(items[i]!, i);
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, lane));
}

/** What a script sends, statement by statement (a Transact-SQL batch up to `GO` as one, as
 *  many times as `GO n` says), and whether any of them may change something. */
export interface ScriptPlan {
  statements: string[];
  writes: boolean;
}

/** Why a script cannot run on several connections: parameters and client commands need the
 *  tab (its values, its files), which each connection's run does not have. */
export type ScriptProblem = "empty" | "parameters" | "commands";

export function planScript(text: string, analysis: SqlAnalysis): ScriptPlan | ScriptProblem {
  const spans = analysis.statements;
  if (spans.length === 0) return "empty";
  if (spans.some((s) => s.kind === "command")) return "commands";
  if (spans.some((s) => s.parameters.length > 0)) return "parameters";
  const groups = analysis.batches.length > 0 ? analysis.batches : spans.map((_, i) => ({ first: i, end: i + 1, repeat: 1 }));
  const statements: string[] = [];
  for (const { first, end, repeat } of groups) {
    const sql = text.slice(spans[first]!.start, spans[end - 1]!.end);
    for (let i = 0; i < Math.max(1, repeat); i++) statements.push(sql);
  }
  return { statements, writes: spans.some((s) => !s.read_only) };
}
