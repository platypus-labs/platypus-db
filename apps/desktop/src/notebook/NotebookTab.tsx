import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import {
  api,
  type CompletionCatalog,
  type ResultPartial,
  type ResultTable,
  type SavedConnection,
  type StatementSpan,
  toCommandError,
} from "../api";
import { parseConfig } from "../analysis/chartConfig";
import { svgText } from "../analysis/chartExport";
import { loadAll, noteZones } from "../analysis/QueryWithSql";
import { resultCatalog, settled, withEvent } from "../analysis/sqlField";
import { describe, type Result } from "../components/ResultPanel";
import { withDriver } from "../drivers/DriverPopup";
import { parameterKeys, substitute } from "../editor/params";
import { datedFileName } from "../format";
import { check } from "../guards/guards";
import { answerFor } from "../testAnswers";
import { GuardDialog } from "../guards/GuardDialog";
import { t } from "../i18n";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { type ExportCell, notebookHtml, writtenParam } from "./html";
import { type CellRun, idle, NotebookCellView } from "./NotebookCell";
import {
  cellNames,
  copyHolds,
  missingParams,
  type NotebookCell,
  newCell,
  notebookFile,
  parseNotebookFile,
  planLines,
  resultTables,
} from "./model";
import { ask } from "../ui/ask";
import { EmptyState } from "../ui/State";
import { toast } from "../ui/toast";
import { Icon } from "../ui/Icon";

interface NotebookTabModel extends TabBase {
  kind: "notebook";
  cells: NotebookCell[];
  /** The values of the parameters (`:name`, `?`, `${name}`) the cells share. */
  params: Record<string, string>;
  /** The `.pnb` file it was opened from or saved to. */
  file: string | null;
}

declare module "../tabs/registry" {
  interface TabKinds {
    notebook: NotebookTabModel;
  }
}

/** Rows of each result the HTML export writes. */
const EXPORT_ROWS = 5000;

/** The in-memory copy of the cells' rows that cells on the results query, and its session. */
interface Copy {
  connection: SavedConnection;
  sessionId: number;
  tables: ResultTable[];
  partial: ResultPartial | null;
}

let nextResult = -1_000_000;

/** What went wrong, as the cell says it: the notebook's own errors without "Error:". */
const reason = (e: unknown) => (e instanceof Error ? e.message : toCommandError(e).message);

/** A cell's statement, ready to run: placeholders relative to its text. */
const statementOf = (text: string, s: StatementSpan) => ({
  sql: text.slice(s.start, s.end),
  span: s,
  params: s.parameters.map((p) => ({ ...p, start: p.start - s.start, end: p.end - s.start })),
});

type Statement = ReturnType<typeof statementOf>;

/** Asks before risky statements (UPDATE without WHERE, production changes), as the editor does. */
function confirmed(request: Parameters<typeof GuardDialog>[0]["request"]): Promise<boolean> {
  return new Promise((resolve) =>
    workbench().openDialog((close) => (
      <GuardDialog
        request={request}
        onAnswer={(run) => {
          close();
          resolve(run);
        }}
      />
    )),
  );
}

const fileName = (path: string) => path.split(/[\\/]/).pop()!.replace(/\.pnb$/i, "");

const pnbFilters = () => [{ name: t("Notebook"), extensions: ["pnb"] }];

/** Opens a `.pnb` file in a notebook on `connection`. */
export async function openNotebook(connection: SavedConnection) {
  const path = await answerFor("file", () => open({ filters: pnbFilters() }));
  if (typeof path !== "string") return;
  try {
    const content = parseNotebookFile(await api.readTextFile(path), parseConfig);
    workbench().openTab("notebook", connection.id, { ...content, file: path, title: fileName(path) });
  } catch (e) {
    workbench().showError(t("{path} is not a notebook: {error}", { path, error: toCommandError(e).message }));
  }
}

/** A notebook: cells of SQL and Markdown run one by one or all in order, each with its result
 *  as a grid or a chart and its plan; cells on the results read the others' rows. */
export function NotebookTab({ tab, connection, active, ensureSession, sideSession, catalog, onNeedCatalog, onChange }: TabProps<NotebookTabModel>) {
  const [runs, setRuns] = useState<Record<string, CellRun>>({});
  const [editing, setEditing] = useState<ReadonlySet<string>>(() => new Set(tab.cells.filter((c) => c.kind === "markdown" && !c.text.trim()).map((c) => c.id)));
  const [runningAll, setRunningAll] = useState(false);
  // "Stop" was pressed during "Run all": the next cells do not start.
  const stopped = useRef(false);
  // Cells whose Stop came before their query had a job: it is cancelled once it has one.
  const stopAsked = useRef(new Set<string>());
  const copy = useRef<Copy | null>(null);
  // The copy is made or loaded again by one cell at a time.
  const copying = useRef<Promise<unknown>>(Promise.resolve());
  const session = useRef<number | null>(null);
  // What the runs are now, ahead of the render: "Run all" reads a cell's rows right after it.
  const runsNow = useRef<Record<string, CellRun>>({});
  const tabNow = useRef(tab);
  tabNow.current = tab;
  const names = cellNames(tab.cells);

  // The jobs and the copy go with the tab.
  useEffect(
    () => () => {
      for (const run of Object.values(runsNow.current)) if (run.result?.jobId != null) void api.closeQuery(run.result.jobId);
      const c = copy.current;
      if (c) void api.closeSession(c.sessionId).catch(() => undefined).then(() => api.closeResultDb(c.connection.id));
    },
    [],
  );

  const setRun = (id: string, change: (run: CellRun) => Partial<CellRun>) => {
    const run = runsNow.current[id] ?? idle;
    runsNow.current = { ...runsNow.current, [id]: { ...run, ...change(run) } };
    setRuns(runsNow.current);
  };

  /** Changes the cells; a notebook kept in a file has changes not saved. */
  const edit = (patch: Partial<Pick<NotebookTabModel, "cells" | "params">>) =>
    onChange({ ...patch, unsaved: tabNow.current.file !== null });

  const changeCell = (id: string, patch: Partial<NotebookCell>) =>
    edit({ cells: tabNow.current.cells.map((c) => (c.id === id ? { ...c, ...patch } : c)) });

  /** The tab's session, in autocommit: a notebook has no Commit button. */
  async function serverSession(): Promise<number | null> {
    const sid = await ensureSession();
    if (sid !== null && session.current !== sid) {
      session.current = sid;
      await api.setAutocommit(sid, true).catch(() => {});
    }
    return sid;
  }

  /** The jobs of the cells whose rows are there. */
  const loadedJobs = () =>
    new Map(
      Object.entries(runsNow.current).flatMap(([id, run]) =>
        run.result?.jobId != null && run.result.status.kind === "rows" ? [[id, run.result.jobId] as [string, number]] : [],
      ),
    );

  /** The session on the copy of the other cells' rows `cell` reads, made or loaded again when
   *  they changed; `null` when cancelled. */
  function resultsSession(cell: NotebookCell): Promise<number | null> {
    const next = copying.current.then(() => copyFor(cell));
    copying.current = next.catch(() => undefined);
    return next;
  }

  async function copyFor(cell: NotebookCell): Promise<number | null> {
    const jobs = loadedJobs();
    const wanted = resultTables(tabNow.current.cells, jobs, cell.id);
    if (wanted.length === 0) throw new Error(t("No other cell has rows yet: run one first."));
    const had = copy.current;
    if (had && copyHolds(had.tables, wanted)) return had.sessionId;
    // Every cell's rows, so that the next cell on the results finds its own too.
    const tables = resultTables(tabNow.current.cells, jobs);
    if (!(await loadAll(tables.map((x) => x.job_id)))) return null;
    if (had) {
      had.partial = await api.reloadResultDb(had.connection.id, tables);
      had.tables = tables;
      return had.sessionId;
    }
    // DuckDB holds the rows: downloaded first if it is not yet.
    const opened = await withDriver(() => api.openResultDb(t("Notebook on {name}", { name: connection.name }), tables));
    const made = await api.openSession(opened.connection.id).catch((e: unknown) => {
      void api.closeResultDb(opened.connection.id);
      throw e;
    });
    copy.current = { connection: { ...opened.connection, read_only: true }, sessionId: made.session_id, tables, partial: opened.partial };
    noteZones(opened.zones);
    return made.session_id;
  }

  /** Runs `sql` on `sid` as `cell`'s result, until it has started, failed or ended. */
  function runStatement(cell: string, sid: number, sql: string): Promise<Result> {
    return new Promise((resolve) => {
      let result: Result = { id: nextResult--, sql, offset: 0, jobId: null, columns: null, rows: 0, fetching: false, status: { kind: "running" }, limit: null };
      // Shown, and settled once started, failed or ended with its job known; a run refused
      // before it had one ends too. The promise keeps the first.
      const update = (next: Result) => {
        result = next;
        setRun(cell, () => ({ result }));
        if (settled(result) || result.status.kind === "error") resolve(result);
      };
      update(result);
      api.runQuery(sid, sql, null, (event) => update(withEvent(result, event)), true).then(
        (jobId) => {
          update({ ...result, jobId });
          if (stopAsked.current.has(cell)) void api.cancelQuery(jobId).catch(() => undefined);
        },
        (e: unknown) => update({ ...result, status: { kind: "error", error: toCommandError(e) } }),
      );
    });
  }

  /** The statements of `cell` with the parameters filled in; `null` (and the reason shown)
   *  when one has no value. */
  async function prepared(cell: NotebookCell): Promise<Statement[]> {
    const driver = cell.onResults ? "duckdb" : connection.driver;
    const statements = (await api.analyzeSql(cell.text, driver, 0)).statements.map((s) => statementOf(cell.text, s));
    if (statements.some((s) => s.span.kind === "command")) throw new Error(t("Client commands (@set, @include…) do not run in a notebook."));
    const keys = [...new Set(statements.flatMap((s) => parameterKeys(s.params)))];
    const params = tabNow.current.params;
    const missing = missingParams(keys, params);
    if (missing.length) {
      edit({ params: { ...Object.fromEntries(missing.map((k) => [k, ""])), ...params } });
      // As the cell writes them: `${name}` is the same value as `:name`, put in as text.
      const written = new Map(
        statements.flatMap((s) => {
          const names = parameterKeys(s.params);
          return s.params.map((p, i) => [names[i]!, p.kind === "variable" ? `\${${names[i]!}}` : writtenParam(names[i]!)] as [string, string]);
        }),
      );
      throw new Error(t("Give a value to {names} above.", { names: missing.map((k) => written.get(k) ?? writtenParam(k)).join(", ") }));
    }
    return statements.map((s) => ({ ...s, sql: s.params.length ? substitute(s.sql, s.params, params, driver) : s.sql }));
  }

  /** The session `cell` runs on, after the checks of its statements; `null` when cancelled. */
  async function sessionFor(cell: NotebookCell, statements: Statement[]): Promise<number | null> {
    if (cell.onResults) return resultsSession(cell);
    if (connection.read_only && statements.some((s) => !s.span.read_only)) {
      throw new Error(t("{name} is read-only: this cell may change data.", { name: connection.name }));
    }
    const request = await check(connection, statements.map((s) => ({ sql: s.sql, span: s.span })), sideSession);
    if (request && !(await confirmed(request))) return null;
    return serverSession();
  }

  /** Runs the cell `id`, as its text is now; resolves false when it failed or was cancelled. */
  async function runCell(id: string): Promise<boolean> {
    const cell = tabNow.current.cells.find((c) => c.id === id);
    if (cell?.kind !== "sql" || !cell.text.trim()) return true;
    const before = runsNow.current[cell.id]?.result?.jobId;
    stopAsked.current.delete(cell.id);
    setRun(cell.id, () => ({ running: true, error: null }));
    try {
      const statements = await prepared(cell);
      const sid = await sessionFor(cell, statements);
      if (sid === null) {
        setRun(cell.id, () => ({ running: false }));
        return false;
      }
      if (before != null) void api.closeQuery(before);
      let last: Result | null = null;
      for (const s of statements) {
        // The cell shows its last statement: earlier ones' rows go.
        if (last?.jobId != null) void api.closeQuery(last.jobId);
        if (stopAsked.current.has(cell.id)) break;
        last = await runStatement(cell.id, sid, s.sql);
        if (last.status.kind === "error") break;
      }
      setRun(cell.id, () => ({ running: false }));
      const ok = last?.status.kind !== "error";
      if (ok && cell.plan) await loadPlan(cell, statements, sid);
      return ok;
    } catch (e) {
      setRun(cell.id, () => ({ running: false, error: reason(e) }));
      return false;
    }
  }

  /** The plan of `cell`'s last statement; nothing runs. */
  async function loadPlan(cell: NotebookCell, statements?: Statement[], sid?: number | null) {
    try {
      const planned = statements ?? (await prepared(cell));
      const last = planned.at(-1);
      if (!last) return;
      // Only explained, not run: no guard to pass.
      const on = sid ?? (cell.onResults ? await resultsSession(cell) : await serverSession());
      if (on === null) return;
      const plan = await api.explain(on, last.sql, false);
      setRun(cell.id, () => ({ plan, planError: null }));
    } catch (e) {
      setRun(cell.id, () => ({ plan: null, planError: t("No plan: {error}", { error: reason(e) }) }));
    }
  }

  /** Cancels the statement `id` is running, with the cancel the SQL editor uses. */
  function stopCell(id: string) {
    stopAsked.current.add(id);
    const result = runsNow.current[id]?.result;
    if (result?.jobId != null && result.status.kind === "running") void api.cancelQuery(result.jobId).catch(() => undefined);
  }

  function stopAll() {
    stopped.current = true;
    for (const [id, run] of Object.entries(runsNow.current)) if (run.running) stopCell(id);
  }

  async function runAll() {
    stopped.current = false;
    setRunningAll(true);
    try {
      for (const cell of tabNow.current.cells) {
        if (stopped.current || !(await runCell(cell.id))) break;
      }
    } finally {
      setRunningAll(false);
    }
  }

  const togglePlan = (cell: NotebookCell) => {
    changeCell(cell.id, { plan: !cell.plan });
    if (!cell.plan) void loadPlan(tabNow.current.cells.find((c) => c.id === cell.id) ?? cell);
  };

  const addCell = (kind: NotebookCell["kind"], after?: string) => {
    const cell = newCell(kind);
    const cells = [...tabNow.current.cells];
    const at = after ? cells.findIndex((c) => c.id === after) + 1 : cells.length;
    cells.splice(at, 0, cell);
    edit({ cells });
    if (kind === "markdown") setEditing((e) => new Set(e).add(cell.id));
  };

  const moveCell = (id: string, by: -1 | 1) => {
    const cells = [...tabNow.current.cells];
    const i = cells.findIndex((c) => c.id === id);
    const j = i + by;
    if (j < 0 || j >= cells.length) return;
    [cells[i], cells[j]] = [cells[j]!, cells[i]!];
    edit({ cells });
  };

  const deleteCell = async (id: string) => {
    // A cell with a text or a result is asked about; an empty one goes at once.
    const cell = tabNow.current.cells.find((c) => c.id === id);
    if (cell && (cell.text.trim() || runsNow.current[id]?.result)) {
      if (!(await ask(t("Delete this cell, with its text and result?"), { confirm: t("Delete"), danger: true }))) return;
    }
    const job = runsNow.current[id]?.result?.jobId;
    if (job != null) void api.closeQuery(job);
    const { [id]: _, ...rest } = runsNow.current;
    runsNow.current = rest;
    setRuns(rest);
    edit({ cells: tabNow.current.cells.filter((c) => c.id !== id) });
  };

  async function saveFile() {
    const path =
      tab.file ??
      (await answerFor("file", () =>
        save({ filters: pnbFilters(), defaultPath: datedFileName(t("Notebook"), "pnb") }),
      ));
    if (!path) return;
    try {
      await api.writeTextFile(path, notebookFile(tab));
      onChange({ file: path, title: fileName(path), unsaved: false });
      toast(t("Notebook saved"));
    } catch (e) {
      toast(toCommandError(e).message, "error");
    }
  }

  /** The cells as the export writes them: rows, chart and plan as shown. */
  async function exportCells(): Promise<ExportCell[]> {
    return Promise.all(
      tab.cells.map(async (cell): Promise<ExportCell> => {
        if (cell.kind === "markdown") return { kind: "markdown", text: cell.text };
        const run = runsNow.current[cell.id] ?? idle;
        const result = run.result;
        let table: Extract<ExportCell, { kind: "sql" }>["table"] = null;
        if (result?.jobId != null && result.status.kind === "rows" && result.columns) {
          const page = await api.fetchRows(result.jobId, 0, EXPORT_ROWS).catch(() => null);
          if (page) {
            table = {
              columns: result.columns.map((c) => c.name),
              rows: Array.from({ length: page.rows }, (_, r) => page.columns.map((col) => col[r] ?? null)),
              // A table that is not the whole result says so, instead of passing for it.
              note:
                result.rows > page.rows
                  ? t("Only the first {shown} of {total} rows are in this export.", { shown: page.rows.toLocaleString(), total: result.rows.toLocaleString() })
                  : null,
            };
          }
        }
        const svg = cell.chart?.shown ? document.querySelector<SVGSVGElement>(`[data-cell="${cell.id}"] .notebook-result svg`) : null;
        return {
          kind: "sql",
          name: names.get(cell.id) ?? "",
          sql: cell.text,
          table,
          summary: run.error ?? (result ? describe(result) : t("Not run")),
          failed: !!run.error || result?.status.kind === "error",
          chart: svg ? svgText(svg) : null,
          plan: !cell.plan ? null : run.plan ? planLines(run.plan.root) : run.planError ? [{ depth: 0, operation: run.planError, detail: null, rows: null, cost: null }] : null,
        };
      }),
    );
  }

  async function exportHtml() {
    const title = tab.title || t("Notebook on {name}", { name: connection.name });
    const path = await answerFor("file", () =>
      save({ filters: [{ name: "HTML", extensions: ["html"] }], defaultPath: datedFileName(title, "html") }),
    );
    if (!path) return;
    try {
      await api.writeTextFile(path, notebookHtml(title, await exportCells(), tab.params));
      toast(t("Exported to {name}", { name: path.split(/[\\/]/).pop()! }));
    } catch (e) {
      toast(toCommandError(e).message, "error");
    }
  }

  const catalogOf = (cell: NotebookCell): CompletionCatalog | null => {
    if (!cell.onResults) return catalog;
    const results = runsNow.current;
    return resultCatalog(
      tab.cells.flatMap((c) => {
        const columns = results[c.id]?.result?.columns;
        const name = names.get(c.id);
        return c.id !== cell.id && columns && name ? [{ table: name, columns }] : [];
      }),
    );
  };

  const paramKeys = Object.keys(tab.params);
  return (
    <div className="object-tab notebook-tab" style={{ display: active ? "flex" : "none" }}>
      <div className="toolbar notebook-toolbar">
        <button type="button" className="primary" disabled={runningAll} onClick={() => void runAll()} data-action="run_all">
          {t("Run all")}
        </button>
        {runningAll && (
          <button type="button" onClick={stopAll} data-action="stop_all">
            {t("Stop")}
          </button>
        )}
        <button type="button" onClick={() => addCell("sql")} data-action="add_sql">
          {t("+ SQL")}
        </button>
        <button type="button" onClick={() => addCell("markdown")} data-action="add_text">
          {t("+ Text")}
        </button>
        <span className="spacer" />
        {copy.current?.partial && <span className="notice warn small">{t("Partial: reading stopped at the memory limit.")}</span>}
        <button type="button" className="boxed" onClick={() => void saveFile()} data-action="save" title={t("Save (Ctrl+S)")}>
          {t("Save")}
        </button>
        <button type="button" className="boxed" onClick={() => void exportHtml()} data-action="export_html" title={t("Export the notebook as a single HTML page")}>
          {t("Export HTML…")}
        </button>
      </div>
      {paramKeys.length > 0 && (
        <div className="notebook-params" aria-label={t("Parameters")}>
          <span className="muted small">{t("Parameters")}</span>
          {paramKeys.map((key) => (
            <label key={key} className="notebook-param">
              <code>{writtenParam(key)}</code>
              <input
                value={tab.params[key] ?? ""}
                data-param={key}
                spellCheck={false}
                onChange={(e) => edit({ params: { ...tab.params, [key]: e.target.value } })}
              />
              <button
                type="button"
                className="icon"
                aria-label={t("Remove")}
                title={t("Remove")}
                onClick={() => edit({ params: Object.fromEntries(Object.entries(tab.params).filter(([k]) => k !== key)) })}
              >
                <Icon name="close" />
              </button>
            </label>
          ))}
        </div>
      )}
      <div className="notebook-cells">
        {tab.cells.map((cell, i) => (
          <NotebookCellView
            key={cell.id}
            cell={cell}
            name={names.get(cell.id) ?? null}
            run={runs[cell.id] ?? idle}
            first={i === 0}
            last={i === tab.cells.length - 1}
            editing={editing.has(cell.id)}
            driver={cell.onResults ? "duckdb" : connection.driver}
            catalog={catalogOf(cell)}
            connectionName={connection.name}
            onChange={(patch) => changeCell(cell.id, patch)}
            onRun={() => void runCell(cell.id)}
            onStop={() => stopCell(cell.id)}
            onPlan={() => togglePlan(cell)}
            onEdit={(on) =>
              setEditing((e) => {
                const next = new Set(e);
                if (on) next.add(cell.id);
                else next.delete(cell.id);
                return next;
              })
            }
            onMove={(by) => moveCell(cell.id, by)}
            onDelete={() => void deleteCell(cell.id)}
            onSave={() => void saveFile()}
            onFocus={() => onNeedCatalog(false)}
          />
        ))}
        {tab.cells.length === 0 && <EmptyState className="notebook-empty" title={t("An empty notebook: add a SQL or a text cell.")} />}
      </div>
    </div>
  );
}
