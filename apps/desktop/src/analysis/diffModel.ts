/** What "Compare with previous run" decides in the UI: same statement, same columns, the
 *  result to keep, the summary. Pure: the unit tests import it. */

export interface ColumnLike {
  name: string;
  db_type: string;
}

interface ResultLike {
  sql: string;
  jobId: number | null;
  columns: readonly ColumnLike[] | null;
  status: { kind: string };
}

/** The statement as compared: spacing and a final semicolon do not matter. */
export function normalizeSql(sql: string): string {
  return sql.replace(/\s+/g, " ").trim().replace(/;$/, "").trimEnd();
}

export const sameStatement = (a: string, b: string) => normalizeSql(a) === normalizeSql(b);

/** Same column names and types, in the same order. */
export function sameColumns(a: readonly ColumnLike[], b: readonly ColumnLike[]): boolean {
  return a.length === b.length && a.every((c, i) => c.name === b[i]?.name && c.db_type === b[i]?.db_type);
}

/** The result with rows of `sql` among `results`: the one a new run of it replaces. */
export function previousOf(results: readonly ResultLike[], sql: string): { sql: string; jobId: number; columns: ColumnLike[] } | null {
  for (const r of results) {
    if (r.status.kind === "rows" && r.jobId !== null && r.columns && sameStatement(r.sql, sql)) {
      return { sql: r.sql, jobId: r.jobId, columns: [...r.columns] };
    }
  }
  return null;
}

/** `+12 −3 ~5`: rows added, removed and changed. */
export function summary(d: { added: number; removed: number; changed: number }): string {
  const n = (v: number) => v.toLocaleString();
  return `+${n(d.added)} −${n(d.removed)} ~${n(d.changed)}`;
}
