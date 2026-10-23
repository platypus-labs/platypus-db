import { useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  api,
  toCommandError,
  type ExportOptions,
  type QueryEvent,
  type TransactionState,
} from "../api";
import { type RunMode, SqlEditor, type SqlEditorHandle } from "../editor/SqlEditor";
import { defineSetting, getSetting } from "../settings";
import { type Sort, viewerSql } from "../sqlNames";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { basename } from "../workspace";
import { ExportDialog, type ExportSource } from "./ExportDialog";
import { HistoryPanel } from "./HistoryPanel";
import { type Result, ResultPanel, type ResultStatus, describe, fetchAll, fmt, formatMs } from "./ResultPanel";

/** Statements that change which tables and columns exist. */
const DDL = /^\s*(create|alter|drop|rename)\b/i;

const scriptRowLimit = defineSetting({
  key: "editor.script_row_limit",
  section: "SQL editor",
  label: "Rows fetched per statement when running a script",
  description: "The rest of each result is not fetched, so long scripts stay fast.",
  default: 1000,
  control: { type: "number", min: 1, step: 100 },
});

/** A SQL editor, or with `table` set, the data viewer of a table. */
export interface QueryTabModel extends TabBase {
  kind: "query";
  sql: string;
  filePath: string | null;
  savedSql: string | null;
  /** Runs the SQL once when the tab opens ("Open data"); not persisted. */
  autorun?: boolean;
  /** Data viewer: the table or view the tab browses, as a quoted, qualified name. */
  table?: string | null;
  /** Data viewer: `WHERE` clause typed by the user. */
  filter?: string;
  /** Data viewer: server-side sort. */
  sort?: Sort | null;
}

declare module "../tabs/registry" {
  interface TabKinds {
    query: QueryTabModel;
  }
}

type Props = TabProps<QueryTabModel>;

interface Planned {
  sql: string;
  offset: number;
  readOnly: boolean;
}

let nextResultId = 1;

function isTerminal(status: ResultStatus, limited: boolean) {
  switch (status.kind) {
    case "affected":
    case "error":
      return true;
    case "rows":
      // Without a limit the job pauses on demand, so the first rows are the end of "running".
      return limited ? status.finished : true;
    default:
      return false;
  }
}

interface ExportProgress {
  exportId: number | null;
  rows: number;
  /** Final message; `null` while running. */
  done: string | null;
  failed: boolean;
}

export function QueryTab({
  tab,
  connection,
  active,
  ensureSession,
  sideSession,
  catalog,
  onNeedCatalog,
  onChange,
  onError,
}: Props) {
  const editor = useRef<SqlEditorHandle>(null);
  const [tx, setTx] = useState<TransactionState | null>(null);
  const [filter, setFilter] = useState(tab.filter ?? "");
  const [count, setCount] = useState<string | null>(null);
  const [exportOpen, setExportOpen] = useState(false);
  const [exporting, setExporting] = useState<ExportProgress | null>(null);
  const [results, setResults] = useState<Result[]>([]);
  const [activeResult, setActiveResult] = useState(0);
  const [running, setRunning] = useState(false);
  const [blocked, setBlocked] = useState<RunMode | null>(null);
  const [showHistory, setShowHistory] = useState(false);
  const [historyVersion, setHistoryVersion] = useState(0);
  const [editorHeight, setEditorHeight] = useState(200);
  const runToken = useRef(0);
  const jobs = useRef<number[]>([]);
  const currentJob = useRef<number | null>(null);

  const closeJobs = () => {
    for (const id of jobs.current) void api.closeQuery(id);
    jobs.current = [];
  };
  // Release results when the tab closes.
  useEffect(() => closeJobs, []);

  // Without a password prompt the catalog can load right away; otherwise on first run.
  useEffect(() => {
    if (connection.save_password || connection.driver === "sqlite") onNeedCatalog(false);
  }, [connection.id]);

  useEffect(() => {
    if (!tab.autorun) return;
    onChange({ autorun: false });
    void run("statement");
  }, []);

  const update = (id: number, patch: Partial<Result>) =>
    setResults((all) => all.map((r) => (r.id === id ? { ...r, ...patch } : r)));

  /** What to run: the selection, the statement under the cursor, or every statement. */
  async function plan(mode: RunMode): Promise<Planned[]> {
    const ed = editor.current!;
    const text = ed.getText();
    const { from, to } = ed.getSelection();
    const scope = from !== to ? { text: text.slice(from, to), offset: from } : { text, offset: 0 };
    const analysis = await api.analyzeSql(scope.text, connection.driver, from - scope.offset);
    const toPlanned = (s: { start: number; end: number; read_only: boolean }) => ({
      sql: scope.text.slice(s.start, s.end),
      offset: scope.offset + s.start,
      readOnly: s.read_only,
    });
    if (mode === "script") return analysis.statements.map(toPlanned);
    if (from !== to) {
      // A selection runs as typed, even if it holds several statements.
      const readOnly = analysis.statements.every((s) => s.read_only);
      return scope.text.trim() ? [{ sql: scope.text, offset: from, readOnly }] : [];
    }
    const at = analysis.at_cursor;
    const statement = at === null ? undefined : analysis.statements[at];
    return statement ? [toPlanned(statement)] : [];
  }

  /** Runs one statement and resolves with its first terminal status. */
  function runOne(sessionId: number, result: Result, token: number): Promise<ResultStatus> {
    const limited = result.limit !== null;
    return new Promise((resolve) => {
      let settled = false;
      const settle = (status: ResultStatus) => {
        if (!settled && isTerminal(status, limited)) {
          settled = true;
          resolve(status);
        }
      };
      const onEvent = (event: QueryEvent) => {
        if (token !== runToken.current) return settle({ kind: "skipped" });
        let status: ResultStatus | null = null;
        switch (event.type) {
          case "started":
            status = { kind: "rows", finished: false, complete: true, elapsedMs: event.elapsed_ms };
            update(result.id, { columns: event.columns, status });
            break;
          case "progress":
            update(result.id, { rows: event.rows });
            break;
          case "finished":
            status = {
              kind: "rows",
              finished: true,
              complete: event.complete,
              stopped: event.stopped,
              elapsedMs: event.elapsed_ms,
            };
            update(result.id, { rows: event.rows, status });
            break;
          case "affected":
            status = { kind: "affected", rows: event.rows, elapsedMs: event.elapsed_ms };
            update(result.id, { status });
            break;
          case "failed":
            status = { kind: "error", error: event.error };
            update(result.id, { status });
            break;
        }
        if (status) settle(status);
      };
      api.runQuery(sessionId, result.sql, result.limit, onEvent).then(
        (jobId) => {
          jobs.current.push(jobId);
          currentJob.current = jobId;
          update(result.id, { jobId });
        },
        (e: unknown) => {
          const status: ResultStatus = { kind: "error", error: toCommandError(e) };
          update(result.id, { status });
          settle(status);
        },
      );
    });
  }

  function showError(planned: Planned, status: ResultStatus) {
    if (status.kind !== "error" || status.error.kind === "cancelled") return;
    const text = editor.current!.getText();
    const position = status.error.position;
    const from = position ? planned.offset + position - 1 : planned.offset;
    const word = /^[\w$"]+/.exec(text.slice(from));
    editor.current!.setError({
      from,
      to: position ? from + (word?.[0].length ?? 1) : planned.offset + planned.sql.length,
      message: status.error.message,
    });
  }

  async function run(mode: RunMode, force = false) {
    if (running && !force) return;
    let planned: Planned[];
    try {
      planned = await plan(mode);
    } catch (e) {
      onError(toCommandError(e).message);
      return;
    }
    if (planned.length === 0) return;
    if (connection.read_only && !force && planned.some((p) => !p.readOnly)) {
      setBlocked(mode);
      return;
    }
    setBlocked(null);
    const sessionId = await ensureSession();
    if (sessionId === null) return;
    onNeedCatalog(false);

    const token = ++runToken.current;
    closeJobs();
    const script = mode === "script" && planned.length > 1;
    const initial: Result[] = planned.map((p, i) => ({
      id: nextResultId++,
      sql: p.sql,
      offset: p.offset,
      jobId: null,
      columns: null,
      rows: 0,
      status: { kind: i === 0 ? "running" : "pending" },
      limit: script ? getSetting(scriptRowLimit) : null,
    }));
    setResults(initial);
    setActiveResult(0);
    setRunning(true);
    editor.current!.setError(null);

    // Afterwards show the failed statement, else the last result set, else the last result.
    let failed = -1;
    let lastRows = -1;
    for (let i = 0; i < initial.length; i++) {
      const result = initial[i]!;
      const p = planned[i]!;
      if (token !== runToken.current) return;
      update(result.id, { status: { kind: "running" } });
      setActiveResult(i);
      editor.current!.flash(p.offset, p.offset + p.sql.length);
      let status = await runOne(sessionId, result, token);
      // A dropped connection fails before the statement reaches the server; the second
      // attempt runs on a reopened connection.
      if (status.kind === "error" && status.error.kind === "closed" && token === runToken.current) {
        update(result.id, { status: { kind: "running" } });
        status = await runOne(sessionId, result, token);
      }
      if (token !== runToken.current) return;
      if (status.kind === "rows") lastRows = i;
      if (status.kind === "error") {
        failed = i;
        showError(p, status);
        const skipped = new Set(initial.slice(i + 1).map((r) => r.id));
        setResults((all) => all.map((r) => (skipped.has(r.id) ? { ...r, status: { kind: "skipped" } } : r)));
        break;
      }
    }
    setActiveResult(failed >= 0 ? failed : lastRows >= 0 ? lastRows : initial.length - 1);
    setRunning(false);
    currentJob.current = null;
    if (planned.some((p) => DDL.test(p.sql))) onNeedCatalog(true);
    setHistoryVersion((v) => v + 1);
    await refreshTx(sessionId);
  }

  async function refreshTx(sessionId: number) {
    try {
      const state = await api.transactionState(sessionId);
      setTx(state);
      if (state.open !== !!tab.txOpen) onChange({ txOpen: state.open });
    } catch (e) {
      onError(toCommandError(e).message);
    }
  }

  async function toggleAutocommit() {
    const sessionId = await ensureSession();
    if (sessionId === null) return;
    try {
      const current = tx ?? (await api.transactionState(sessionId));
      setTx(await api.setAutocommit(sessionId, !current.autocommit));
    } catch (e) {
      onError(toCommandError(e).message);
    }
  }

  async function endTransaction(commit: boolean) {
    const sessionId = await ensureSession();
    if (sessionId === null) return;
    try {
      const state = await api.endTransaction(sessionId, commit);
      setTx(state);
      onChange({ txOpen: state.open });
    } catch (e) {
      onError(toCommandError(e).message);
      await refreshTx(sessionId);
    }
  }

  /** Data viewer: rebuilds the query from filter and sort, then runs it. */
  function applyViewer(patch: { filter?: string; sort?: Sort | null }) {
    if (!tab.table) return;
    const next = { filter: patch.filter ?? tab.filter ?? "", sort: patch.sort !== undefined ? patch.sort : (tab.sort ?? null) };
    onChange(next);
    setCount(null);
    editor.current!.setText(viewerSql(connection.driver, tab.table, next.filter, next.sort));
    void run("statement");
  }

  /** Header clicks cycle a column through ascending, descending and unsorted. */
  function sortBy(column: string) {
    const sort = tab.sort ?? null;
    if (sort?.column !== column) applyViewer({ sort: { column, desc: false } });
    else if (!sort.desc) applyViewer({ sort: { column, desc: true } });
    else applyViewer({ sort: null });
  }

  async function countRows() {
    if (!tab.table) return;
    setCount("counting…");
    const sessionId = await sideSession();
    if (sessionId === null) return setCount(null);
    const where = tab.filter?.trim() ? ` WHERE ${tab.filter.trim()}` : "";
    try {
      const value = await api.queryValue(sessionId, `SELECT COUNT(*) FROM ${tab.table}${where}`);
      setCount(value === null ? null : `${fmt.format(Number(value))} rows`);
    } catch (e) {
      setCount(null);
      onError(toCommandError(e).message);
    }
  }

  async function startExport(result: Result, path: string, options: ExportOptions, source: ExportSource) {
    setExportOpen(false);
    const file = basename(path);
    const started = performance.now();
    const finish = (done: string, failed = false) =>
      setExporting((e) => ({ exportId: null, rows: e?.rows ?? 0, done, failed }));
    if (source === "loaded") {
      if (result.jobId === null) return;
      setExporting({ exportId: null, rows: result.rows, done: null, failed: false });
      try {
        const rows = await api.exportRows(result.jobId, path, options);
        finish(`Exported ${fmt.format(rows)} rows to ${file} in ${formatMs(Math.round(performance.now() - started))}`);
      } catch (e) {
        finish(`Export failed: ${toCommandError(e).message}`, true);
      }
      return;
    }
    const sessionId = await ensureSession();
    if (sessionId === null) return;
    setExporting({ exportId: null, rows: 0, done: null, failed: false });
    try {
      const exportId = await api.exportQuery(sessionId, result.sql, path, options, (event) => {
        switch (event.type) {
          case "progress":
            setExporting((e) => e && { ...e, rows: event.rows });
            break;
          case "done":
            finish(`Exported ${fmt.format(event.rows)} rows to ${file} in ${formatMs(event.elapsed_ms)}`);
            break;
          case "failed":
            finish(`Export failed: ${event.error.message}`, true);
            break;
          case "cancelled":
            finish("Export cancelled");
            break;
        }
      });
      setExporting((e) => e && e.done === null ? { ...e, exportId } : e);
    } catch (e) {
      finish(`Export failed: ${toCommandError(e).message}`, true);
    }
  }

  async function cancel() {
    const job = currentJob.current;
    if (job === null) return;
    try {
      await api.cancelQuery(job);
    } catch (e) {
      onError(toCommandError(e).message);
    }
  }

  async function saveFile() {
    const sql = editor.current!.getText();
    let path = tab.filePath;
    if (!path) {
      path = await save({ filters: [{ name: "SQL", extensions: ["sql"] }], defaultPath: "query.sql" });
      if (!path) return;
    }
    try {
      await api.writeTextFile(path, sql);
      onChange({ filePath: path, savedSql: sql });
    } catch (e) {
      onError(toCommandError(e).message);
    }
  }

  async function openFile() {
    const path = await open({ filters: [{ name: "SQL", extensions: ["sql", "txt"] }], multiple: false });
    if (typeof path !== "string") return;
    try {
      const sql = await api.readTextFile(path);
      workbench().openTab("query", connection.id, { sql, filePath: path });
    } catch (e) {
      onError(toCommandError(e).message);
    }
  }

  function startResize(e: React.MouseEvent) {
    e.preventDefault();
    const startY = e.clientY;
    const start = editorHeight;
    const onMove = (ev: MouseEvent) =>
      setEditorHeight(Math.max(60, Math.min(window.innerHeight - 200, start + ev.clientY - startY)));
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  const current = results[activeResult];
  const dirty = tab.filePath !== null && tab.sql !== tab.savedSql;
  const rowsResult = current?.status.kind === "rows" && current.columns ? current : null;
  const manual = tx !== null && !tx.autocommit;
  const exportBusy = exporting !== null && exporting.done === null;

  return (
    <div className="query-tab" style={{ display: active ? "flex" : "none" }}>
      <div
        className="query-main"
        style={{ gridTemplateRows: `auto ${tab.table ? "auto " : ""}${editorHeight}px 6px 1fr auto` }}
      >
        <div className="toolbar">
          <button className="primary" onClick={() => void run("statement")} title="Run statement (Ctrl+Enter)">
            ▶ Run
          </button>
          <button onClick={() => void run("script")} title="Run script (Alt+X, Ctrl+Shift+Enter)">
            ▶▶ Script
          </button>
          <button onClick={() => void cancel()} disabled={!running}>
            ■ Cancel
          </button>
          <span className="divider" />
          <button
            className={manual ? "toggled" : ""}
            onClick={() => void toggleAutocommit()}
            disabled={running || !!tx?.open}
            title={
              tx?.open
                ? "Commit or roll back before switching"
                : manual
                  ? "Manual commit: changes wait for Commit"
                  : "Auto-commit: every statement commits on its own"
            }
          >
            {manual ? "Manual commit" : "Auto-commit"}
          </button>
          {manual && (
            <>
              <button onClick={() => void endTransaction(true)} disabled={running || !tx.open}>
                Commit
              </button>
              <button onClick={() => void endTransaction(false)} disabled={running || !tx.open}>
                Rollback
              </button>
            </>
          )}
          {tx?.open && (
            <span className="tx-open" title="A transaction is open: changes are not visible to others yet">
              ● Transaction open
            </span>
          )}
          <span className="divider" />
          <button onClick={() => void openFile()} title="Open script (Ctrl+O)">
            Open
          </button>
          <button onClick={() => void saveFile()} title="Save script (Ctrl+S)">
            Save{dirty ? " •" : ""}
          </button>
          <span className="spacer" />
          {tab.filePath && <span className="muted small">{basename(tab.filePath)}</span>}
          <button className={showHistory ? "toggled" : ""} onClick={() => setShowHistory((s) => !s)}>
            History
          </button>
        </div>
        {tab.table && (
          <form
            className="viewer-bar"
            onSubmit={(e) => {
              e.preventDefault();
              applyViewer({ filter });
            }}
          >
            <span className="muted small">WHERE</span>
            <input
              value={filter}
              placeholder="e.g. total > 100 AND status = 'paid'"
              onChange={(e) => setFilter(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape" && filter) {
                  setFilter("");
                  applyViewer({ filter: "" });
                }
              }}
            />
            <button type="submit">Apply</button>
            {tab.sort && (
              <button type="button" className="link" onClick={() => applyViewer({ sort: null })}>
                Sorted by {tab.sort.column} {tab.sort.desc ? "▼" : "▲"} ×
              </button>
            )}
            <span className="spacer" />
            {count && <span className="muted small">{count}</span>}
            <button type="button" onClick={() => void countRows()} title="Runs SELECT COUNT(*) with the filter">
              Count rows
            </button>
          </form>
        )}
        <SqlEditor
          ref={editor}
          initialValue={tab.sql}
          driver={connection.driver}
          catalog={catalog}
          onChange={(sql) => onChange({ sql })}
          onRun={(mode) => void run(mode)}
          onSave={() => void saveFile()}
          onOpen={() => void openFile()}
        />
        <div className="splitter" onMouseDown={startResize} />
        <div className="results">
          {blocked !== null && (
            <div className="notice warn">
              <strong>{connection.name}</strong> is read-only and this may modify data.
              <button onClick={() => void run(blocked, true)}>Run anyway</button>
            </div>
          )}
          {results.length > 1 && (
            <div className="result-tabs">
              {results.map((r, i) => (
                <button
                  key={r.id}
                  className={`result-tab ${r.status.kind}${i === activeResult ? " active" : ""}`}
                  onClick={() => setActiveResult(i)}
                  title={r.sql}
                >
                  {i + 1}. {describe(r)}
                </button>
              ))}
            </div>
          )}
          {current && (
            <ResultPanel
              key={current.id}
              result={current}
              sort={tab.table ? (tab.sort ?? null) : null}
              onHeaderClick={tab.table && !running ? sortBy : undefined}
              source={{ connection, sql: current.sql, jobId: current.jobId, table: tab.table ?? null }}
            />
          )}
        </div>
        <div className="statusbar">
          {current && (
            <>
              <span>{describe(current)}</span>
              {(current.status.kind === "rows" || current.status.kind === "affected") && (
                <span className="muted">
                  {current.status.kind === "rows" && !current.status.finished ? "first rows" : "took"}{" "}
                  {formatMs(current.status.elapsedMs)}
                </span>
              )}
              {current.status.kind === "rows" && !current.status.finished && (
                <button className="link" onClick={() => fetchAll(current)}>
                  Fetch all
                </button>
              )}
              {rowsResult && !exportBusy && (
                <button className="link" onClick={() => setExportOpen(true)}>
                  Export…
                </button>
              )}
            </>
          )}
          {exporting && (
            <span className={exporting.failed ? "error-text" : "muted"}>
              {exporting.done ?? `Exporting… ${fmt.format(exporting.rows)} rows`}
              {exportBusy && exporting.exportId !== null && (
                <button className="link" onClick={() => void api.cancelExport(exporting.exportId!)}>
                  Cancel
                </button>
              )}
              {!exportBusy && (
                <button className="link" onClick={() => setExporting(null)} title="Dismiss">
                  ×
                </button>
              )}
            </span>
          )}
          <span className="spacer" />
          <span className="muted">Ctrl+Enter statement · Alt+X script · {connection.name}</span>
        </div>
      </div>
      {showHistory && (
        <HistoryPanel
          connectionId={connection.id}
          refresh={historyVersion}
          onInsert={(sql) => editor.current?.insert(sql)}
          onClose={() => setShowHistory(false)}
        />
      )}
      {exportOpen && rowsResult && (
        <ExportDialog
          name={tab.title ?? "result"}
          loadedRows={rowsResult.rows}
          complete={rowsResult.status.kind === "rows" && rowsResult.status.finished && rowsResult.status.complete}
          transactionOpen={!!tx?.open}
          onExport={(path, options, source) => void startExport(rowsResult, path, options, source)}
          onClose={() => setExportOpen(false)}
        />
      )}
    </div>
  );
}
