import { useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  api,
  toCommandError,
  type ExportOptions,
  type StatementSpan,
  type QueryEvent,
  type SavedConnection,
  type TableDetails,
  type TransactionState,
} from "../api";
import type { EditSource, TableRef } from "../grid/edit/meta";
import type { TableEditTarget } from "../grid/edit/TableEditor";
import { type RunMode, SqlEditor, type SqlEditorHandle } from "../editor/SqlEditor";
import { defineSetting, getSetting, useSetting } from "../settings";
import { type Sort, viewerSql } from "../sqlNames";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { basename } from "../workspace";
import { type CheckedStatement, type GuardRequest, check, elapsed, transactionWarning } from "../guards/guards";
import { GuardDialog } from "../guards/GuardDialog";
import { ExportDialog, type ExportSource } from "./ExportDialog";
import { HistoryPanel } from "./HistoryPanel";
import { useToolbar } from "./toolbar";
import { Icon } from "../ui/Icon";
import { type Result, ResultPanel, type ResultStatus, describe, fetchAll, fmt, formatMs } from "./ResultPanel";

/** Auto-refresh intervals in seconds, `0` for off. */
const REFRESH_CHOICES: [number, string][] = [
  [0, "Off"],
  [2, "2 s"],
  [5, "5 s"],
  [10, "10 s"],
  [30, "30 s"],
  [60, "1 min"],
  [300, "5 min"],
];

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
  /** Data viewer: the same table by schema and name, for the data editor. */
  tableRef?: TableRef | null;
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
  /** Its statements and what they do, for the checks before running. */
  checks: CheckedStatement[];
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
  /** A run waiting for the answer to its checks. */
  const [guard, setGuard] = useState<{ request: GuardRequest; resolve: (run: boolean) => void } | null>(null);
  /** When the open transaction began, as far as the tab saw it. */
  const [txSince, setTxSince] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const warnMinutes = useSetting(transactionWarning);
  const warned = useRef(false);
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
  /** The data editor has changes that running again would lose. */
  const unsaved = useRef(false);
  /** Data viewer: the table's keys, loaded once and again after DDL. */
  const details = useRef<Promise<TableDetails> | null>(null);
  /** The table each query reads, for editing its results; cleared after DDL. */
  const sources = useRef(new Map<string, Promise<EditSource | null>>());
  /** What the last run ran, run again after saving edited rows. */
  const lastPlanned = useRef<Planned[]>([]);
  /** SQL that undoes the last saved edits. */
  const [undoScript, setUndoScript] = useState<string | null>(null);
  /** Seconds between runs of the last statements again; `null` is off. */
  const [refreshEvery, setRefreshEvery] = useState<number | null>(null);

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
    const checked = (span: StatementSpan) => ({ sql: scope.text.slice(span.start, span.end), span });
    const toPlanned = (s: StatementSpan): Planned => ({
      sql: scope.text.slice(s.start, s.end),
      offset: scope.offset + s.start,
      readOnly: s.read_only,
      checks: [checked(s)],
    });
    if (mode === "script") return analysis.statements.map(toPlanned);
    if (from !== to) {
      // A selection runs as typed, even if it holds several statements.
      const readOnly = analysis.statements.every((s) => s.read_only);
      const checks = analysis.statements.map(checked);
      return scope.text.trim() ? [{ sql: scope.text, offset: from, readOnly, checks }] : [];
    }
    const at = analysis.at_cursor;
    const statement = at === null ? undefined : analysis.statements[at];
    return statement ? [toPlanned(statement)] : [];
  }

  /** Runs one statement and resolves with its first terminal status. */
  function runOne(sessionId: number, result: Result, token: number, history: boolean): Promise<ResultStatus> {
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
      api.runQuery(sessionId, result.sql, result.limit, onEvent, history).then(
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

  /** Runs what `mode` selects in the editor, or `again` (the statements of the last run).
   *  An `auto` refresh stays out of the query history, keeps the result tab shown and the
   *  editor as it is, and stops refreshing when a statement fails. */
  async function run(mode: RunMode, force = false, again?: Planned[], auto = false) {
    const history = !auto;
    if (running && !force) return;
    if (unsaved.current && !window.confirm("Discard the changes to the data that are not saved?")) return;
    let planned: Planned[];
    try {
      planned = again ?? (await plan(mode));
    } catch (e) {
      onError(toCommandError(e).message);
      return;
    }
    if (planned.length === 0) return;
    if (!again) setUndoScript(null);
    lastPlanned.current = planned;
    if (connection.read_only && !force && planned.some((p) => !p.readOnly)) {
      setBlocked(mode);
      return;
    }
    setBlocked(null);
    // Statements of a run asked about already go again without asking.
    if (!again) {
      const request = await check(connection, planned.flatMap((p) => p.checks), sideSession);
      if (request && !(await new Promise<boolean>((resolve) => setGuard({ request, resolve })))) return;
    }
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
    if (!auto) setActiveResult(0);
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
      if (!auto) {
        setActiveResult(i);
        editor.current!.flash(p.offset, p.offset + p.sql.length);
      }
      let status = await runOne(sessionId, result, token, history);
      // A dropped connection fails before the statement reaches the server; the second
      // attempt runs on a reopened connection.
      if (status.kind === "error" && status.error.kind === "closed" && token === runToken.current) {
        update(result.id, { status: { kind: "running" } });
        status = await runOne(sessionId, result, token, history);
      }
      if (token !== runToken.current) return;
      if (status.kind === "rows") lastRows = i;
      if (status.kind === "error") {
        failed = i;
        // The editor may have changed since the statements were planned: no marker there.
        if (!auto) showError(p, status);
        else onError(`Auto-refresh stopped: ${status.error.message}`);
        const skipped = new Set(initial.slice(i + 1).map((r) => r.id));
        setResults((all) => all.map((r) => (skipped.has(r.id) ? { ...r, status: { kind: "skipped" } } : r)));
        break;
      }
    }
    if (auto && failed >= 0) setRefreshEvery(null);
    if (!auto || failed >= 0) setActiveResult(failed >= 0 ? failed : lastRows >= 0 ? lastRows : initial.length - 1);
    setRunning(false);
    currentJob.current = null;
    if (planned.some((p) => DDL.test(p.sql))) {
      onNeedCatalog(true);
      details.current = null;
      sources.current.clear();
    }
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
    if (unsaved.current) {
      if (!window.confirm("Discard the changes to the data that are not saved?")) return;
      unsaved.current = false;
    }
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
      workbench().openTab("query", connection.id, { sql, filePath: path, database: tab.database });
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

  function setUnsaved(dirty: boolean) {
    unsaved.current = dirty;
    if (dirty !== !!tab.unsaved) onChange({ unsaved: dirty });
  }

  async function sideId(): Promise<number> {
    const id = await sideSession();
    if (id === null) throw new Error("no connection");
    return id;
  }

  /** The table a query reads, found on the navigator session; `null` when there is none. */
  async function querySource(sql: string): Promise<EditSource | null> {
    const query = await api.querySource(sql, connection.driver);
    if (!query) return null;
    const id = await sideId();
    const schema = query.schema ?? (await api.listSchemas(id)).find((s) => s.current)?.name;
    if (!schema) return null;
    const objects = await api.listObjects(id, schema);
    const object =
      objects.find((o) => o.name === query.name) ??
      objects.find((o) => o.name.toLowerCase() === query.name.toLowerCase());
    if (!object) return null;
    const table: TableRef = { schema, name: object.name, kind: object.kind };
    return { table, details: await api.tableDetails(id, schema, object.name), items: query.items };
  }

  /**
   * Lets the grid edit the result's table: the data viewer's own query, or a query in the
   * editor that reads one table. Results of scripts stay read-only.
   */
  function editTarget(result: Result): TableEditTarget | null {
    const ref = tab.tableRef;
    const viewer =
      !!tab.table &&
      !!ref &&
      result.sql === viewerSql(connection.driver, tab.table, tab.filter ?? "", tab.sort ?? null);
    if (!viewer && results.length !== 1) return null;
    const forget = <T,>(p: Promise<T>, drop: () => void) =>
      p.catch((e: unknown) => {
        drop();
        throw e;
      });
    const load = (): Promise<EditSource | null> => {
      if (viewer) {
        details.current ??= sideId().then((id) => api.tableDetails(id, ref.schema, ref.name));
        return forget(details.current, () => (details.current = null)).then((d) => ({
          table: ref,
          details: d,
          items: null,
        }));
      }
      let source = sources.current.get(result.sql);
      if (!source) {
        source = querySource(result.sql);
        sources.current.set(result.sql, source);
      }
      return forget(source, () => sources.current.delete(result.sql));
    };
    return {
      connection,
      key: viewer ? `table:${tab.table}` : `sql:${result.sql}`,
      quiet: !viewer,
      readOnly: null,
      load,
      session: sideSession,
      onSaved: (undo) => {
        setUnsaved(false);
        setUndoScript(undo);
        void run("statement", false, lastPlanned.current);
      },
      onDirtyChange: setUnsaved,
    };
  }

  // The open transaction's clock, and one warning when it has been open too long.
  useEffect(() => {
    if (!tx?.open) {
      setTxSince(null);
      warned.current = false;
      return;
    }
    setTxSince((since) => since ?? Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [tx?.open]);
  const txAge = txSince === null ? 0 : Math.max(0, now - txSince);
  const txLong = warnMinutes > 0 && txAge >= warnMinutes * 60_000;
  useEffect(() => {
    if (!txLong || warned.current) return;
    warned.current = true;
    onError(`A transaction on ${connection.name} has been open for ${warnMinutes} minutes: commit or roll back.`);
  }, [txLong]);

  // Auto-refresh: the last statements again, if they only read, while the tab is shown and
  // nothing runs, asks or waits to be saved. The timer calls the latest render's `refresh`.
  const refresh = useRef(() => {});
  refresh.current = () => {
    const again = lastPlanned.current;
    if (running || guard || unsaved.current || again.length === 0) return;
    if (again.some((p) => !p.readOnly)) {
      setRefreshEvery(null);
      onError("Auto-refresh stopped: the last statements change data.");
      return;
    }
    void run("statement", false, again, true);
  };
  useEffect(() => {
    if (refreshEvery === null || !active) return;
    const timer = setInterval(() => refresh.current(), refreshEvery * 1000);
    return () => clearInterval(timer);
  }, [refreshEvery, active]);

  const toolbar = useToolbar();
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
          <button
            className="primary"
            onClick={() => void run("statement")}
            title="Run statement (Ctrl+Enter)"
            aria-label="Run"
          >
            {toolbar.label("run", "Run")}
          </button>
          {toolbar.visible.script && (
            <button onClick={() => void run("script")} title="Run script (Alt+X, Ctrl+Shift+Enter)" aria-label="Script">
              {toolbar.label("script", "Script")}
            </button>
          )}
          {toolbar.visible.cancel && (
            <button
              onClick={() => void cancel()}
              disabled={!running}
              title="Cancel the running statement"
              aria-label="Cancel"
            >
              {toolbar.label("cancel", "Cancel")}
            </button>
          )}
          <ConnectionChip connection={connection} database={tab.database ?? null} schema={catalog?.default_schema ?? null} />
          {toolbar.visible.transactions && <span className="divider" />}
          {toolbar.visible.transactions && (
          <button
            className={manual ? "toggled" : ""}
            onClick={() => void toggleAutocommit()}
            disabled={running || !!tx?.open}
            aria-label={manual ? "Manual commit" : "Auto-commit"}
            title={
              tx?.open
                ? "Commit or roll back before switching"
                : manual
                  ? "Manual commit: changes wait for Commit"
                  : "Auto-commit: every statement commits on its own"
            }
          >
            {toolbar.label(manual ? "manual-commit" : "autocommit", manual ? "Manual commit" : "Auto-commit")}
          </button>
          )}
          {toolbar.visible.transactions && manual && (
            <>
              <button
                onClick={() => void endTransaction(true)}
                disabled={running || !tx.open}
                title="Commit"
                aria-label="Commit"
              >
                {toolbar.label("commit", "Commit")}
              </button>
              <button
                onClick={() => void endTransaction(false)}
                disabled={running || !tx.open}
                title="Rollback"
                aria-label="Rollback"
              >
                {toolbar.label("rollback", "Rollback")}
              </button>
            </>
          )}
          {tx?.open && (
            <span
              className={`tx-open${txLong ? " long" : ""}`}
              title="A transaction is open: changes are not visible to others yet, and locks are held"
            >
              ● Transaction open {elapsed(txAge)}
            </span>
          )}
          {toolbar.visible.files && (
            <>
              <span className="divider" />
              <button onClick={() => void openFile()} title="Open script (Ctrl+O)" aria-label="Open">
                {toolbar.label("open", "Open")}
              </button>
              <button onClick={() => void saveFile()} title="Save script (Ctrl+S)" aria-label="Save">
                {toolbar.label("save", "Save")}
                {dirty ? " •" : ""}
              </button>
            </>
          )}
          {toolbar.visible.refresh && (
            <label
              className={`auto-refresh${refreshEvery !== null ? " toggled" : ""}`}
              title="Run the last statements again every few seconds, if they only read data"
            >
              <Icon name="refresh" />
              <select
                aria-label="Auto-refresh"
                value={refreshEvery ?? 0}
                onChange={(e) => setRefreshEvery(Number(e.target.value) || null)}
              >
                {REFRESH_CHOICES.map(([seconds, label]) => (
                  <option key={seconds} value={seconds}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          )}
          <span className="spacer" />
          {tab.filePath && <span className="muted small">{basename(tab.filePath)}</span>}
          {toolbar.visible.history && (
            <button className={showHistory ? "toggled" : ""} onClick={() => setShowHistory((s) => !s)} title="History" aria-label="History">
              {toolbar.label("history", "History")}
            </button>
          )}
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
              source={{
                connection,
                sql: current.sql,
                jobId: current.jobId,
                table: tab.table ?? null,
                viewer: tab.table
                  ? {
                      filter: tab.filter ?? "",
                      setFilter: (next) => {
                        setFilter(next);
                        applyViewer({ filter: next });
                      },
                    }
                  : undefined,
              }}
              edit={editTarget(current)}
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
          {undoScript && (
            <span className="muted">
              Changes saved.
              <button
                className="link"
                onClick={() => workbench().openTab("query", connection.id, { sql: undoScript })}
                title="Opens the SQL that puts the rows back as they were"
              >
                Undo script
              </button>
              <button className="link" onClick={() => setUndoScript(null)} title="Dismiss">
                ×
              </button>
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
      {guard && (
        <GuardDialog
          request={guard.request}
          onAnswer={(run) => {
            guard.resolve(run);
            setGuard(null);
          }}
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

/** Where the editor's statements run: connection, database when not the saved one, schema;
 *  colored by the connection's type. */
function ConnectionChip({ connection, database, schema }: { connection: SavedConnection; database: string | null; schema: string | null }) {
  const parts = [connection.name, database, schema].filter(Boolean);
  return (
    <span className={`conn-chip${connection.connection_type ? ` ${connection.connection_type}` : ""}`} title="Statements run here">
      <span className="conn-chip-dot" />
      {parts.join(" · ")}
    </span>
  );
}
