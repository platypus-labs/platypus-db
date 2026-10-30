import { type ReactNode, useEffect, useState } from "react";
import { api, type ColumnInfo, type CommandError } from "../api";
import type { GridSource } from "../grid/DataGrid";
import { useFormatSignature } from "../grid/formats";
import { type NotesScope, ResultGrid } from "../grid/ResultGrid";
import { RowCache } from "../grid/RowCache";
import { ChartView } from "../analysis/ChartView";
import type { ChartConfig } from "../analysis/chartConfig";
import type { TableEditTarget } from "../grid/edit/TableEditor";
import { formatCount, t, plural } from "../i18n";
import { Icon } from "../ui/Icon";
import { ErrorState } from "../ui/State";

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
  /** The `LIMIT` the editor added to the query (`safety.auto_limit`). */
  autoLimit?: number | null;
  /** What the server said while the statement ran (notices, `PRINT`, warnings), and what the
   *  app has to say about the run (a reopened connection). */
  messages?: string[];
}

/** Counts as the interface's language groups them (see `formatCount`). */
export const fmt = { format: formatCount };

export function formatMs(ms: number) {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(2)} s`;
}

/** One-line summary of a result, for tabs and the status bar. */
export function describe(result: Result): string {
  const s = result.status;
  switch (s.kind) {
    case "pending":
      return t("Waiting");
    case "running":
      return t("Executing…");
    case "rows": {
      const rows = plural(result.rows, "{n} row", "{n} rows");
      if (!s.finished) return t("{rows} loaded · more available", { rows });
      if (s.stopped) return t("{rows} · fetching stopped", { rows });
      if (!s.complete && result.limit !== null && result.rows >= result.limit) {
        return t("first {rows} (script limit)", { rows });
      }
      if (!s.complete) return t("{rows} · stopped at the memory limit", { rows });
      if (result.autoLimit && result.rows >= result.autoLimit) return t("first {rows} (automatic LIMIT)", { rows });
      return rows;
    }
    case "affected":
      return plural(s.rows, "{n} row affected", "{n} rows affected");
    case "error":
      return t(s.error.kind === "cancelled" ? "Cancelled" : "Error");
    case "skipped":
      return t("Not executed");
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
  /** Why the headers do not sort, as their tooltip. */
  headerHint?: string;
  /** Passed to the grid's actions. */
  source?: GridSource | null;
  /** The table whose rows the result shows, to edit them. */
  edit?: TableEditTarget | null;
  notes?: NotesScope | null;
  /** Shown as a chart instead of the grid: what it draws, and where a change of it goes. */
  chart?: { config: ChartConfig | null; onConfig: (config: ChartConfig) => void; bare?: boolean } | null;
  /** Above the grid or the chart: the result's SQL field. */
  toolbar?: ReactNode;
}

/** The messages of a result, above it. */
function Messages({ lines }: { lines: string[] | undefined }) {
  if (!lines?.length) return null;
  return (
    <pre className="result-message server-messages" aria-label={t("Messages")}>
      {lines.join("\n")}
    </pre>
  );
}

export function ResultPanel(props: PanelProps) {
  return (
    <>
      <Messages lines={props.result.messages} />
      <ResultBody {...props} />
    </>
  );
}

/** The error as plain text, for the clipboard. */
function errorText(e: CommandError): string {
  return [`${e.code ? `${e.code}: ` : ""}${e.message}`, e.detail, e.hint].filter(Boolean).join("\n");
}

function ResultBody({ result, sort, onHeaderClick, headerHint, source, edit, notes, chart, toolbar }: PanelProps) {
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
      <ErrorState
        className="result-error"
        title={`${e.code ? `${e.code}: ` : ""}${e.message}`}
        actions={
          <button type="button" onClick={() => void navigator.clipboard.writeText(errorText(e))}>
            <Icon name="copy" size={14} /> {t("Copy error")}
          </button>
        }
      >
        {e.detail && <span className="state-line">{t("DETAIL: {text}", { text: e.detail })}</span>}
        {e.hint && <span className="state-line">{t("HINT: {text}", { text: e.hint })}</span>}
      </ErrorState>
    );
  }
  if (status.kind === "affected") {
    return (
      <div className="notice ok">
        {plural(status.rows, "{n} row affected in {time}", "{n} rows affected in {time}", { time: formatMs(status.elapsedMs) })}
      </div>
    );
  }
  if (status.kind === "skipped") {
    return <div className="notice muted">{t("Not executed: an earlier statement failed.")}</div>;
  }
  if (status.kind === "message") {
    return <pre className={`message-output${status.failed ? " failed" : ""}`}>{status.text}</pre>;
  }
  // A thin bar while the statement runs or rows are on their way.
  const loading =
    status.kind === "running" || (status.kind === "rows" && !status.finished && result.fetching) || !!cache?.loading;
  const bar = loading && <div className="result-loading" role="progressbar" aria-label={t("Loading rows")} />;
  if (!(columns && cache && jobId !== null)) {
    return status.kind === "running" ? (
      <>
        {bar}
        <div className="notice muted result-running">{t("Executing…")}</div>
      </>
    ) : null;
  }
  return (
    <>
      {bar}
      {cache.error && (
        <div className="notice error small" role="alert">
          {t("Rows could not be read: {message}", { message: cache.error })}
        </div>
      )}
      {chart && toolbar && <div className="edit-bar">{toolbar}</div>}
      {chart ? (
        <ChartView
          jobId={jobId}
          columns={columns}
          rows={result.rows}
          complete={status.kind === "rows" && status.finished && status.complete}
          config={chart.config}
          onConfig={chart.onConfig}
          bare={chart.bare}
        />
      ) : (
        <ResultGrid
          jobId={jobId}
          columns={columns}
          rowCount={result.rows}
          cache={cache}
          version={version}
          sort={sort}
          onHeaderClick={onHeaderClick && ((i) => onHeaderClick(columns[i]!.name))}
          headerHint={headerHint}
          source={source}
          edit={edit}
          notes={notes}
          toolbar={toolbar}
          empty={status.kind === "rows" && status.finished && result.rows === 0}
        />
      )}
    </>
  );
}

export function fetchAll(result: Result) {
  if (result.jobId !== null) void api.fetchAll(result.jobId);
}
