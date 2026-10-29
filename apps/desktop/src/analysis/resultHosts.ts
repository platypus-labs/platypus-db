/**
 * The SQL editor tabs whose results "Query with SQL" reads, and the tables it makes of them.
 * Pure apart from the registry: the unit tests import it.
 */
import type { ResultPartial, ResultTable, SavedConnection } from "../api";

/** Where the tables of a "Query with SQL" tab came from. */
export interface ResultSource {
  /** The tab whose results were copied, for "Reload from result". */
  tabKey: number;
  /** The tables, `result` first. */
  tables: string[];
  /** Set when a table lacks rows of its result, with why. */
  partial: ResultPartial | null;
}

/** A result with rows: its job and its place among the tab's results (1 for the first). */
export interface ResultRows {
  jobId: number;
  position: number;
}

/** A SQL editor tab as "Query with SQL" sees it. */
export interface ResultHost {
  tabKey: number;
  connection: SavedConnection;
  /** The tab is on screen: the palette's command works on it. */
  shown(): boolean;
  results(): ResultRows[];
  /** The result shown, if it has a job. */
  activeJob(): number | null;
  /** Puts the focus in the SQL field of the result shown. */
  querySql(): void;
  /** Opens the pivot form for the result shown. */
  pivot(): void;
}

const hosts = new Set<ResultHost>();

/** Makes a tab's results available until the returned function is called. */
export function provideResults(host: ResultHost): () => void {
  hosts.add(host);
  return () => {
    hosts.delete(host);
  };
}

export const shownHost = (): ResultHost | null => [...hosts].find((h) => h.shown()) ?? null;
export const hostOfTab = (tabKey: number): ResultHost | null => [...hosts].find((h) => h.tabKey === tabKey) ?? null;
export const hostOfJob = (jobId: number): ResultHost | null =>
  [...hosts].find((h) => h.results().some((r) => r.jobId === jobId)) ?? null;

/** The results with rows of a tab's list, with their place in it. */
export function resultsWithRows(results: readonly { jobId: number | null; status: { kind: string } }[]): ResultRows[] {
  return results.flatMap((r, i) => (r.status.kind === "rows" && r.jobId !== null ? [{ jobId: r.jobId, position: i + 1 }] : []));
}

/** The tables to make: the chosen result (else the last) as `result`; with several results,
 *  each also as `r<position>`, numbered as the result tabs are. */
export function resultTables(results: readonly ResultRows[], chosen: number | null): ResultTable[] {
  const main = results.find((r) => r.jobId === chosen) ?? results.at(-1);
  if (!main) return [];
  const tables: ResultTable[] = [{ job_id: main.jobId, table: "result" }];
  if (results.length > 1) for (const r of results) tables.push({ job_id: r.jobId, table: `r${r.position}` });
  return tables;
}

/** The editor's first text in the new tab: the query of `result`, after the tables' names. */
export function initialSql(tables: readonly ResultTable[]): string {
  const names = tables.map((t) => t.table);
  return names.length > 1 ? `-- ${names.join(", ")}\nSELECT * FROM result` : "SELECT * FROM result";
}
