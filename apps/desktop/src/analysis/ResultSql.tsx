/** SQL on a result: a one-line field in the bar above a result's grid. Enter copies the rows
 *  of the tab's results into an in-memory DuckDB (once, and again after the tab ran again),
 *  runs the field's query there, and the grid shows its output, read-only, in place of the
 *  result; × or Escape returns to the result and frees the copy. Each statement of the tab has
 *  its own field. */
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { api, type QueryEvent, type ResultPartial, type ResultTable, type SavedConnection, toCommandError } from "../api";
import type { Result } from "../components/ResultPanel";
import { SqlEditor, type SqlEditorHandle } from "../editor/SqlEditor";
import { SqlLine, type SqlLineHandle } from "../editor/SqlLine";
import { msg, plural, t } from "../i18n";
import { Icon } from "../ui/Icon";
import { loadAll, queryWithSql } from "./QueryWithSql";
import { hostOfJob, resultTables, resultsWithRows } from "./resultHosts";
import { oneLine, resultCatalog, resultQuery, settled, staleCopy, withEvent } from "./sqlField";

/** The in-memory copy behind a field and the session that queries it. */
interface Copy {
  connection: SavedConnection;
  sessionId: number;
  tables: ResultTable[];
  partial: ResultPartial | null;
}

/** A query of the field and the job of the result it was run on. */
interface Run {
  result: Result;
  of: number;
}

/** What a field shows. */
export interface FieldState {
  text: string;
  /** The query running, until it has started or failed. */
  pending: Run | null;
  /** Shown in place of the result, while that is still the run it was made from. */
  output: Run | null;
  error: string | null;
  busy: boolean;
  /** This run loaded the copy again, as the result ran again. */
  reloaded: boolean;
  partial: ResultPartial | null;
}

const EMPTY: FieldState = { text: "", pending: null, output: null, error: null, busy: false, reloaded: false, partial: null };

let nextId = -1;

const closeJob = (run: Run | null) => {
  if (run?.result.jobId != null) void api.closeQuery(run.result.jobId);
};

/** The fields of a tab, by statement, with their copies; `changed` renders again. */
class Fields {
  private states = new Map<string, FieldState>();
  private copies = new Map<string, Copy>();
  private tokens = new Map<string, number>();
  private token = 0;

  constructor(
    private changed: () => void,
    private name: () => string,
  ) {}

  get(key: string): FieldState {
    return this.states.get(key) ?? EMPTY;
  }

  private set(key: string, patch: Partial<FieldState>) {
    this.states.set(key, { ...this.get(key), ...patch });
    this.changed();
  }

  setText(key: string, text: string) {
    if (text !== this.get(key).text) this.set(key, { text });
  }

  /** Runs `text` on the copy of `tables`, `of` being the result's job and `columns` its
   *  column names. */
  async run(key: string, text: string, tables: ResultTable[], of: number, columns: readonly string[]) {
    const sql = resultQuery(text, columns);
    if (sql === null) return this.clear(key);
    const token = ++this.token;
    this.tokens.set(key, token);
    this.set(key, { text, busy: true, error: null });
    try {
      const copy = await this.copy(key, tables, token);
      if (copy === null) {
        if (this.tokens.get(key) === token) this.set(key, { busy: false });
        return;
      }
      this.start(key, token, copy.copy, sql, of, copy.reloaded);
    } catch (e) {
      if (this.tokens.get(key) === token) this.set(key, { busy: false, error: toCommandError(e).message });
    }
  }

  /** The copy of `tables`, made or loaded again when needed; `null` when the load was
   *  cancelled or another run (or clear) came meanwhile. */
  private async copy(key: string, tables: ResultTable[], token: number): Promise<{ copy: Copy; reloaded: boolean } | null> {
    const had = this.copies.get(key);
    if (had && !staleCopy(had.tables, tables)) return { copy: had, reloaded: false };
    if (!(await loadAll(tables.map((x) => x.job_id)))) return null;
    if (had) {
      had.partial = await api.reloadResultDb(had.connection.id, tables);
      had.tables = tables;
      return { copy: had, reloaded: true };
    }
    const opened = await api.openResultDb(t("Results of {name}", { name: this.name() }), tables);
    const connection = { ...opened.connection, read_only: true };
    const session = await api.openSession(connection.id).catch((e: unknown) => {
      void api.closeResultDb(connection.id);
      throw e;
    });
    const copy: Copy = { connection, sessionId: session.session_id, tables, partial: opened.partial };
    if (this.tokens.get(key) !== token) {
      free(copy);
      return null;
    }
    this.copies.set(key, copy);
    return { copy, reloaded: false };
  }

  private start(key: string, token: number, copy: Copy, sql: string, of: number, reloaded: boolean) {
    const result: Result = {
      id: nextId--,
      sql,
      offset: 0,
      jobId: null,
      columns: null,
      rows: 0,
      fetching: false,
      status: { kind: "running" },
      limit: null,
    };
    const before = this.get(key);
    closeJob(before.pending);
    // The session runs one statement at a time: the output shown keeps the rows it has read.
    if (before.output?.result.jobId != null) void api.stopFetch(before.output.result.jobId);
    this.set(key, { pending: { result, of }, reloaded, partial: copy.partial });
    const live = () => this.tokens.get(key) === token;
    // Once shown, the output takes its events until it goes, also while a newer query runs.
    const onEvent = (event: QueryEvent) => {
      if (live() || this.get(key).output?.result.id === result.id) this.advance(key, result.id, (r) => withEvent(r, event));
    };
    api.runQuery(copy.sessionId, sql, null, onEvent, false).then(
      (jobId) => {
        if (live()) this.advance(key, result.id, (r) => ({ ...r, jobId }));
        else void api.closeQuery(jobId);
      },
      (e: unknown) => {
        if (live()) this.set(key, { pending: null, busy: false, error: toCommandError(e).message });
      },
    );
  }

  /** Applies `change` to the query `id`: the output's rows as they arrive, or the pending
   *  query, which becomes the output once it has started. */
  private advance(key: string, id: number, change: (r: Result) => Result) {
    const state = this.get(key);
    if (state.output?.result.id === id) {
      return this.set(key, { output: { ...state.output, result: change(state.output.result) } });
    }
    if (state.pending?.result.id !== id) return;
    const pending = { ...state.pending, result: change(state.pending.result) };
    if (!settled(pending.result)) return this.set(key, { pending });
    const status = pending.result.status;
    if (status.kind !== "rows") {
      closeJob(pending);
      const error = status.kind === "error" ? status.error.message : t("Only a query that returns rows can be shown.");
      return this.set(key, { pending: null, busy: false, error });
    }
    closeJob(state.output);
    this.set(key, { pending: null, output: pending, busy: false, error: null });
  }

  /** Back to the result: the text, the output and the copy go. */
  clear(key: string) {
    const state = this.get(key);
    this.tokens.delete(key);
    closeJob(state.pending);
    closeJob(state.output);
    const copy = this.copies.get(key);
    if (copy) free(copy);
    this.copies.delete(key);
    this.states.delete(key);
    this.changed();
  }

  /** The result ran again (its job is now `jobId`): an output of the old rows goes. */
  forget(key: string, jobId: number | null) {
    const state = this.get(key);
    if (!state.output || state.output.of === jobId) return;
    closeJob(state.output);
    this.set(key, { output: null, reloaded: false });
  }

  dispose() {
    for (const key of [...new Set([...this.states.keys(), ...this.copies.keys()])]) this.clear(key);
  }

  copyOf(key: string): Copy | null {
    return this.copies.get(key) ?? null;
  }
}

/** Closes the session on a copy, then the copy. */
function free(copy: Copy) {
  void api
    .closeSession(copy.sessionId)
    .catch(() => undefined)
    .then(() => api.closeResultDb(copy.connection.id));
}

/** What the tab shows for its result: the output of the field's query in its place. */
export interface ResultSqlView {
  /** The output to show in place of the shown result; `null` shows the result. */
  output: Result | null;
  /** The copy's read-only connection, for the grid's actions, and its session, where Export
   *  runs the query again. */
  connection: SavedConnection | null;
  sessionId: number | null;
  /** The field, for the bar above the grid; `null` when the shown result has no rows. */
  bar: ReactNode;
  /** Puts the focus in the field. */
  focus(): void;
}

interface Options {
  connection: SavedConnection;
  results: Result[];
  shown: Result | undefined;
  /** MongoDB and terminal mode have no field. */
  enabled: boolean;
  /** Asked before the output takes the place of the result: whether its unsaved edits can go. */
  canLeave: () => Promise<boolean>;
}

/** The fields of a tab's results; the shown result's one is `bar`. */
export function useResultSql({ connection, results, shown, enabled, canLeave }: Options): ResultSqlView {
  const [, render] = useState(0);
  const name = useRef(connection.name);
  name.current = connection.name;
  const [fields] = useState(() => new Fields(() => render((n) => n + 1), () => name.current));
  useEffect(() => () => fields.dispose(), []);
  const field = useRef<SqlLineHandle>(null);
  /** The field had the focus: it takes it again when the grid under it changes. */
  const keepFocus = useRef(false);
  const [expanded, setExpanded] = useState(false);

  const key = enabled && shown?.status.kind === "rows" && shown.jobId !== null ? shown.sql : null;
  const state = key === null ? EMPTY : fields.get(key);
  useEffect(() => {
    if (key !== null) fields.forget(key, shown?.jobId ?? null);
  }, [key, shown?.jobId]);
  useEffect(() => setExpanded(false), [key]);

  const tables = useMemo(
    () => (shown?.jobId == null ? [] : resultTables(resultsWithRows(results), shown.jobId)),
    [results, shown?.jobId],
  );
  const named = tables.flatMap((x) => {
    const columns = results.find((r) => r.jobId === x.job_id)?.columns;
    return columns ? [{ table: x.table, columns }] : [];
  });
  // The results change as their rows arrive, their tables and columns once per run.
  const catalogKey = named.map((x) => `${x.table}:${x.columns.map((c) => c.name).join(",")}`).join(";");
  const catalog = useMemo(() => resultCatalog(named), [catalogKey]);
  /** The result's column names, which `GROUP BY` ordinals in the field name. */
  const columnNames = named.find((x) => x.table === "result")?.columns.map((c) => c.name) ?? [];

  const output = state.output && state.output.of === shown?.jobId ? state.output.result : null;
  const copy = output && key !== null ? fields.copyOf(key) : null;

  const run = async (text: string) => {
    if (key === null || shown?.jobId == null) return;
    const of = shown.jobId;
    keepFocus.current = true;
    if (resultQuery(text) !== null && !output && !(await canLeave())) return;
    await fields.run(key, text, tables, of, columnNames);
  };
  const clear = () => {
    if (key === null) return;
    keepFocus.current = true;
    setExpanded(false);
    fields.clear(key);
  };
  const openInTab = (text: string) => {
    setExpanded(false);
    const host = shown?.jobId == null ? null : hostOfJob(shown.jobId);
    if (host) void queryWithSql(host, shown?.jobId ?? undefined, resultQuery(text, columnNames) ?? undefined);
  };

  const bar =
    key === null ? null : (
      <div
        className="result-sql"
        onBlur={(e) => {
          // Focus moved elsewhere; a field that goes away with its grid has no next target.
          if (e.relatedTarget && !e.currentTarget.contains(e.relatedTarget as Node)) keepFocus.current = false;
        }}
      >
        <div className="result-sql-field" title={t(FIELD_TITLE)}>
          <Icon name="sql" />
          <SqlLine
            ref={field}
            value={oneLine(state.text)}
            driver="duckdb"
            catalog={catalog}
            placeholder={t("SQL on the loaded rows: WHERE …, ORDER BY …, GROUP BY … or SELECT … FROM result")}
            label={t("SQL on the result")}
            autoFocus={keepFocus.current}
            onChange={(text) => fields.setText(key, text)}
            onRun={() => void run(state.text)}
            onClear={clear}
            onExpand={() => setExpanded(true)}
          />
          {(state.text !== "" || output) && (
            <button type="button" className="link result-sql-clear" onClick={clear} title={t("Back to the result (Escape)")} aria-label={t("Back to the result")}>
              ×
            </button>
          )}
        </div>
        <button
          type="button"
          className="square"
          onClick={() => setExpanded(!expanded)}
          title={t("Larger editor (Ctrl+Enter)")}
          aria-label={t("Larger editor")}
        >
          <Icon name="expand" />
        </button>
        <FieldStatus state={state} output={output} />
        {expanded && (
          <SqlPopover
            text={state.text}
            catalog={catalog}
            onRun={(text) => {
              setExpanded(false);
              void run(text);
            }}
            onOpenInTab={openInTab}
            onClose={() => {
              setExpanded(false);
              field.current?.focus();
            }}
          />
        )}
      </div>
    );

  return {
    output,
    connection: copy?.connection ?? null,
    sessionId: copy?.sessionId ?? null,
    bar,
    focus: () => field.current?.focus(),
  };
}

const FIELD_TITLE = msg(
  "Runs on a copy of the loaded rows in an in-memory DuckDB, not on the database. A clause or a condition applies to the result; other results of the tab are r1, r2, …. Enter runs, Ctrl+Enter opens a larger editor, Escape returns to the result.",
);

/** After the field: running, the error, or what the grid shows instead of the result. */
function FieldStatus({ state, output }: { state: FieldState; output: Result | null }) {
  if (state.busy) return <span className="muted small">{t("Executing…")}</span>;
  if (state.error) {
    return (
      <span className="error-text small edit-error" title={state.error}>
        {state.error}
      </span>
    );
  }
  if (!output) return null;
  return (
    <span className="result-sql-mark small" title={t("The grid shows the output of the SQL, not the result: × or Escape returns to it.")}>
      {plural(output.rows, "SQL view · {n} row", "SQL view · {n} rows")}
      {state.partial && (
        <span
          className="result-sql-partial"
          title={
            state.partial === "memory_limit"
              ? t("Partial: reading stopped at the memory limit.")
              : t("Partial: reading stopped before the end of the rows.")
          }
        >
          {" · "}
          {t("partial")}
        </span>
      )}
      {state.reloaded && (
        <span className="muted" title={t("The result ran again: its rows were loaded again.")}>
          {" · "}
          {t("reloaded")}
        </span>
      )}
    </span>
  );
}

/** The larger editor under the field, for queries on several lines. */
function SqlPopover({
  text,
  catalog,
  onRun,
  onOpenInTab,
  onClose,
}: {
  text: string;
  catalog: ReturnType<typeof resultCatalog>;
  onRun: (text: string) => void;
  onOpenInTab: (text: string) => void;
  onClose: () => void;
}) {
  const editor = useRef<SqlEditorHandle>(null);
  const current = () => editor.current?.getText() ?? text;
  useEffect(() => editor.current?.focus(), []);
  return (
    <div
      className="result-sql-popover"
      role="dialog"
      aria-label={t("SQL on the result")}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !e.defaultPrevented) {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="result-sql-editor">
        <SqlEditor ref={editor} initialValue={text || "SELECT * FROM result"} driver="duckdb" catalog={catalog} onRun={() => onRun(current())} />
      </div>
      <div className="actions">
        <button type="button" onClick={() => onOpenInTab(current())} title={t("Opens the SQL in a new tab on a copy of the results")}>
          {t("Open in a tab")}
        </button>
        <span className="spacer" />
        <button type="button" onClick={onClose}>
          {t("Close")}
        </button>
        <button type="button" className="primary" onClick={() => onRun(current())} title={t("Run (Ctrl+Enter)")}>
          {t("Run")}
        </button>
      </div>
    </div>
  );
}
