import { useEffect, useState } from "react";
import { api, type ColumnInfo, type CommandError } from "../api";
import { DataGrid } from "../grid/DataGrid";
import { RowCache } from "../grid/RowCache";

export type ResultStatus =
  | { kind: "pending" }
  | { kind: "running" }
  | { kind: "rows"; finished: boolean; complete: boolean; elapsedMs: number }
  | { kind: "affected"; rows: number; elapsedMs: number }
  | { kind: "error"; error: CommandError }
  | { kind: "skipped" };

export interface Result {
  /** Local id, stable across re-renders. */
  id: number;
  sql: string;
  /** Statement start in the editor (UTF-16), to place error positions. */
  offset: number;
  jobId: number | null;
  columns: ColumnInfo[] | null;
  rows: number;
  status: ResultStatus;
  /** Row cap in script mode. */
  limit: number | null;
}

export const fmt = new Intl.NumberFormat();

export function formatMs(ms: number) {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(2)} s`;
}

/** One-line summary of a result, for tabs and the status bar. */
export function describe(result: Result): string {
  const s = result.status;
  switch (s.kind) {
    case "pending":
      return "Waiting";
    case "running":
      return "Executing…";
    case "rows": {
      const rows = `${fmt.format(result.rows)} ${result.rows === 1 ? "row" : "rows"}`;
      if (!s.finished) return `${rows} loaded · more available`;
      if (!s.complete && result.limit !== null && result.rows >= result.limit) {
        return `first ${rows} (script limit)`;
      }
      if (!s.complete) return `${rows} · stopped at the memory limit`;
      return rows;
    }
    case "affected":
      return `${fmt.format(s.rows)} row(s) affected`;
    case "error":
      return s.error.kind === "cancelled" ? "Cancelled" : "Error";
    case "skipped":
      return "Not executed";
  }
}

/** Grid (or message) for one statement's result. */
export function ResultPanel({ result }: { result: Result }) {
  const [cache, setCache] = useState<RowCache | null>(null);
  const [version, setVersion] = useState(0);
  const { jobId, columns, status } = result;

  // One cache per result, created once both the job id and the schema are known.
  useEffect(() => {
    if (jobId === null || columns === null) return;
    const c = new RowCache(jobId, () => setVersion((v) => v + 1));
    setCache(c);
    return () => {
      c.dispose();
      setCache(null);
    };
  }, [jobId, columns]);

  if (status.kind === "error") {
    const e = status.error;
    return (
      <div className="notice error result-error">
        <div>
          <strong>{e.code ? `${e.code}: ` : ""}</strong>
          {e.message}
        </div>
        {e.detail && <div>DETAIL: {e.detail}</div>}
        {e.hint && <div>HINT: {e.hint}</div>}
      </div>
    );
  }
  if (status.kind === "affected") {
    return (
      <div className="notice ok">
        {fmt.format(status.rows)} row(s) affected in {formatMs(status.elapsedMs)}
      </div>
    );
  }
  if (status.kind === "skipped") {
    return <div className="notice muted">Not executed: an earlier statement failed.</div>;
  }
  if (columns && cache) {
    return <DataGrid columns={columns} rowCount={result.rows} cache={cache} version={version} />;
  }
  return null;
}

export function fetchAll(result: Result) {
  if (result.jobId !== null) void api.fetchAll(result.jobId);
}
