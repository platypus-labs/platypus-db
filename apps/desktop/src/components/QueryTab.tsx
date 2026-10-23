import { useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, toCommandError, type QueryEvent, type SavedConnection } from "../api";
import { type RunMode, SqlEditor, type SqlEditorHandle } from "../editor/SqlEditor";
import { basename } from "../workspace";
import { HistoryPanel } from "./HistoryPanel";
import { type Result, ResultPanel, type ResultStatus, describe, fetchAll, formatMs } from "./ResultPanel";

/** Rows fetched per statement when running a script; the rest is cancelled. */
const SCRIPT_ROW_LIMIT = 1000;

export interface TabModel {
  key: number;
  connectionId: string;
  sql: string;
  filePath: string | null;
  savedSql: string | null;
  /** Runs the SQL once when the tab opens ("Open data"); not persisted. */
  autorun?: boolean;
  /** Shown instead of the file name, e.g. the table opened from the navigator. */
  title?: string | null;
}

interface Props {
  tab: TabModel;
  connection: SavedConnection;
  active: boolean;
  /** Opens the tab's session on first use; `null` if the user cancelled. */
  ensureSession: () => Promise<number | null>;
  onChange: (patch: Partial<TabModel>) => void;
  onOpenFile: (path: string, sql: string) => void;
  onError: (message: string) => void;
}

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

export function QueryTab({ tab, connection, active, ensureSession, onChange, onOpenFile, onError }: Props) {
  const editor = useRef<SqlEditorHandle>(null);
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
            status = { kind: "rows", finished: true, complete: event.complete, elapsedMs: event.elapsed_ms };
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
      limit: script ? SCRIPT_ROW_LIMIT : null,
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
      const status = await runOne(sessionId, result, token);
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
    setHistoryVersion((v) => v + 1);
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
      onOpenFile(path, await api.readTextFile(path));
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

  return (
    <div className="query-tab" style={{ display: active ? "flex" : "none" }}>
      <div className="query-main" style={{ gridTemplateRows: `auto ${editorHeight}px 6px 1fr auto` }}>
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
        <SqlEditor
          ref={editor}
          initialValue={tab.sql}
          driver={connection.driver}
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
          {current && <ResultPanel key={current.id} result={current} />}
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
            </>
          )}
          <span className="spacer" />
          <span className="muted">
            Ctrl+Enter statement · Alt+X script · {connection.name}
          </span>
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
    </div>
  );
}
