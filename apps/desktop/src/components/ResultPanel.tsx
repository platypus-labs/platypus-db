import { useEffect, useState } from "react";
import { api, type ColumnInfo, type CommandError } from "../api";
import type { GridSource } from "../grid/DataGrid";
import { useFormatSignature } from "../grid/formats";
import { type NotesScope, ResultGrid } from "../grid/ResultGrid";
import { RowCache } from "../grid/RowCache";
import type { TableEditTarget } from "../grid/edit/TableEditor";

export type ResultStatus =
  | { kind: "pending" }
  | { kind: "running" }
  | { kind: "rows"; finished: boolean; complete: boolean; stopped?: boolean; elapsedMs: number }
  | { kind: "affected"; rows: number; elapsedMs: number }
  | { kind: "error"; error: CommandError }
  | { kind: "skipped" }
  /** Output of the app itself: a client command such as `@echo`, or a command-line client. */
  | { kind: "message"; summary: string; text: string; failed?: boolean };

export interface Result {
  /** Local id, stable across re-renders. */
  id: number;
  sql: string;
  /** Statement start in the editor (UTF-16), to place error positions. */
  offset: number;
  jobId: number | null;
  columns: ColumnInfo[] | null;
  rows: number;
  /** Rows are being read from the server (the job pauses until the grid asks for more). */
  fetching: boolean;
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
      if (s.stopped) return `${rows} · fetching stopped`;
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
    case "message":
      return s.summary;
  }
}

/** Grid (or message) for one statement's result. */
interface PanelProps {
  result: Result;
  sort?: { column: string; desc: boolean } | null;
  /** Header clicks with the column name; the data viewer sorts with them. */
  onHeaderClick?: (column: string) => void;
  /** Passed to the grid's actions. */
  source?: GridSource | null;
  /** The table whose rows the result shows, to edit them. */
  edit?: TableEditTarget | null;
  notes?: NotesScope | null;
}

export function ResultPanel({ result, sort, onHeaderClick, source, edit, notes }: PanelProps) {
  const [cache, setCache] = useState<RowCache | null>(null);
  const [version, setVersion] = useState(0);
  const { jobId, columns, status } = result;

  const formats = useFormatSignature();
  // One cache per result, created once both the job id and the schema are known, and again
  // when the display formats change the rendered text.
  useEffect(() => {
    if (jobId === null || columns === null) return;
    const c = new RowCache(jobId, () => setVersion((v) => v + 1));
    setCache(c);
    return () => {
      c.dispose();
      setCache(null);
    };
  }, [jobId, columns, formats]);

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
  if (status.kind === "message") {
    return <pre className={`message-output${status.failed ? " failed" : ""}`}>{status.text}</pre>;
  }
  // A thin bar while the statement runs or rows are on their way.
  const loading =
    status.kind === "running" || (status.kind === "rows" && !status.finished && result.fetching) || !!cache?.loading;
  const bar = loading && <div className="result-loading" role="progressbar" aria-label="Loading rows" />;
  if (!(columns && cache && jobId !== null)) {
    return status.kind === "running" ? (
      <>
        {bar}
        <div className="notice muted result-running">Executing…</div>
      </>
    ) : null;
  }
  return (
    <>
      {bar}
      <ResultGrid
        jobId={jobId}
        columns={columns}
        rowCount={result.rows}
        cache={cache}
        version={version}
        sort={sort}
        onHeaderClick={onHeaderClick && ((i) => onHeaderClick(columns[i]!.name))}
        source={source}
        edit={edit}
        notes={notes}
      />
    </>
  );
}

export function fetchAll(result: Result) {
  if (result.jobId !== null) void api.fetchAll(result.jobId);
}
