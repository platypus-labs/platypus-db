import { useEffect, useRef, useState } from "react";
import {
  api,
  toCommandError,
  type ColumnInfo,
  type CommandError,
  type QueryEvent,
  type SavedConnection,
} from "../api";
import { DataGrid } from "../grid/DataGrid";
import { RowCache } from "../grid/RowCache";

interface Props {
  sessionId: number;
  connection: SavedConnection;
  active: boolean;
}

type Status =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "rows"; finished: boolean; complete: boolean; elapsedMs: number }
  | { kind: "affected"; rows: number; elapsedMs: number }
  | { kind: "error"; error: CommandError; sql: string; offset: number };

/** Statements allowed on read-only connections without confirmation. */
const READ_ONLY_SAFE = /^\s*(select|with|show|explain|values|table)\b/i;

const fmt = new Intl.NumberFormat();

function formatMs(ms: number) {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(2)} s`;
}

/** 1-based line and column of a 1-based character position. */
function lineCol(text: string, position: number) {
  const before = text.slice(0, position - 1).split("\n");
  return { line: before.length, col: (before.at(-1)?.length ?? 0) + 1 };
}

export function QueryTab({ sessionId, connection, active }: Props) {
  const [sql, setSql] = useState("SELECT * FROM demo.big_table");
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const [columns, setColumns] = useState<ColumnInfo[] | null>(null);
  const [rowCount, setRowCount] = useState(0);
  const [jobId, setJobId] = useState<number | null>(null);
  const [cache, setCache] = useState<RowCache | null>(null);
  const [version, setVersion] = useState(0);
  const [blocked, setBlocked] = useState<string | null>(null);
  const [editorHeight, setEditorHeight] = useState(180);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const runToken = useRef(0);
  const jobRef = useRef<number | null>(null);

  // One cache per result, created once both the job id and the schema are known.
  useEffect(() => {
    if (jobId === null || columns === null) return;
    const c = new RowCache(jobId, () => setVersion((v) => v + 1));
    setCache(c);
    return () => c.dispose();
  }, [jobId, columns]);

  // Release the last result when the tab closes.
  useEffect(
    () => () => {
      if (jobRef.current !== null) void api.closeQuery(jobRef.current);
    },
    [],
  );

  function selectedSql() {
    const el = editorRef.current;
    if (el && el.selectionStart !== el.selectionEnd) {
      return { text: sql.slice(el.selectionStart, el.selectionEnd), offset: el.selectionStart };
    }
    return { text: sql, offset: 0 };
  }

  async function run(force = false) {
    const { text, offset } = selectedSql();
    if (!text.trim()) return;
    if (connection.read_only && !force && !READ_ONLY_SAFE.test(text)) {
      setBlocked(text);
      return;
    }
    setBlocked(null);
    if (jobRef.current !== null) void api.closeQuery(jobRef.current);
    const token = ++runToken.current;
    jobRef.current = null;
    setJobId(null);
    setColumns(null);
    setCache(null);
    setRowCount(0);
    setStatus({ kind: "running" });

    const onEvent = (event: QueryEvent) => {
      if (token !== runToken.current) return;
      switch (event.type) {
        case "started":
          setColumns(event.columns);
          setStatus({ kind: "rows", finished: false, complete: true, elapsedMs: event.elapsed_ms });
          break;
        case "progress":
          setRowCount(event.rows);
          break;
        case "finished":
          setRowCount(event.rows);
          setStatus({
            kind: "rows",
            finished: true,
            complete: event.complete,
            elapsedMs: event.elapsed_ms,
          });
          break;
        case "affected":
          setStatus({ kind: "affected", rows: event.rows, elapsedMs: event.elapsed_ms });
          break;
        case "failed":
          setStatus({ kind: "error", error: event.error, sql: text, offset });
          break;
      }
    };

    try {
      const id = await api.runQuery(sessionId, text, onEvent);
      if (token !== runToken.current) {
        void api.closeQuery(id);
        return;
      }
      jobRef.current = id;
      setJobId(id);
    } catch (e) {
      if (token === runToken.current) {
        setStatus({ kind: "error", error: toCommandError(e), sql: text, offset });
      }
    }
  }

  async function cancel() {
    if (jobRef.current !== null) {
      try {
        await api.cancelQuery(jobRef.current);
      } catch (e) {
        console.error(e);
      }
    }
  }

  // Put the cursor on the error position in the editor.
  useEffect(() => {
    if (status.kind !== "error" || !status.error.position) return;
    const el = editorRef.current;
    if (!el) return;
    const at = status.offset + status.error.position - 1;
    el.focus();
    el.setSelectionRange(at, at + 1);
  }, [status]);

  function onEditorKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      void run();
    } else if (e.key === "Tab") {
      e.preventDefault();
      const el = e.currentTarget;
      const { selectionStart: s, selectionEnd: end } = el;
      setSql(sql.slice(0, s) + "  " + sql.slice(end));
      requestAnimationFrame(() => el.setSelectionRange(s + 2, s + 2));
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

  const running = status.kind === "running" || (status.kind === "rows" && !status.finished);

  return (
    <div className="query-tab" style={{ display: active ? "grid" : "none", gridTemplateRows: `auto ${editorHeight}px 6px 1fr auto` }}>
      <div className="toolbar">
        <button className="primary" onClick={() => void run()} title="Run (Ctrl+Enter)">
          ▶ Run
        </button>
        <button onClick={cancel} disabled={!running}>
          ■ Cancel
        </button>
        <span className="spacer" />
        <span className="muted">Ctrl+Enter runs the selection, or everything</span>
      </div>
      <textarea
        ref={editorRef}
        className="editor"
        spellCheck={false}
        value={sql}
        onChange={(e) => setSql(e.target.value)}
        onKeyDown={onEditorKeyDown}
      />
      <div className="splitter" onMouseDown={startResize} />
      <div className="results">
        {blocked !== null && (
          <div className="notice warn">
            <strong>{connection.name}</strong> is read-only and this statement may modify data.
            <button onClick={() => void run(true)}>Run anyway</button>
          </div>
        )}
        {status.kind === "error" && (
          <div className="notice error result-error">
            <div>
              <strong>{status.error.code ? `${status.error.code}: ` : ""}</strong>
              {status.error.message}
            </div>
            {status.error.detail && <div>DETAIL: {status.error.detail}</div>}
            {status.error.hint && <div>HINT: {status.error.hint}</div>}
            {status.error.position && (
              <div className="muted">
                {(() => {
                  const { line, col } = lineCol(status.sql, status.error.position);
                  return `at line ${line}, column ${col}`;
                })()}
              </div>
            )}
          </div>
        )}
        {status.kind === "affected" && (
          <div className="notice ok">
            {fmt.format(status.rows)} row(s) affected in {formatMs(status.elapsedMs)}
          </div>
        )}
        {columns && cache && (
          <DataGrid columns={columns} rowCount={rowCount} cache={cache} version={version} />
        )}
      </div>
      <div className="statusbar">
        {status.kind === "running" && <span>Executing…</span>}
        {status.kind === "rows" && (
          <>
            <span>
              {fmt.format(rowCount)} rows
              {!status.finished && " loaded · more available"}
              {status.finished && !status.complete && " · stopped at the memory limit"}
            </span>
            <span className="muted">
              {status.finished ? "total" : "first rows"} {formatMs(status.elapsedMs)}
            </span>
            {!status.finished && jobId !== null && (
              <button className="link" onClick={() => void api.fetchAll(jobId)}>
                Fetch all
              </button>
            )}
          </>
        )}
        <span className="spacer" />
        <span className="muted">{connection.name}</span>
      </div>
    </div>
  );
}
