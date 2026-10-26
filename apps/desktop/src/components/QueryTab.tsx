import { useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  api,
  toCommandError,
  type ColumnInfo,
  type CommandError,
  type ExportFormat,
  type ExportOptions,
  type ParameterSpan,
  type StatementSpan,
  type QueryEvent,
  type SavedConnection,
  type TableDetails,
  type TransactionState,
  isMongo,
} from "../api";
import { asksPassword, isProduction } from "../connections";
import type { NotesScope } from "../grid/ResultGrid";
import type { EditSource, TableRef } from "../grid/edit/meta";
import type { TableEditTarget } from "../grid/edit/TableEditor";
import { cliName } from "../editor/actions";
import { OutlinePanel } from "../editor/OutlinePanel";
import { ParamsDialog } from "../editor/ParamsDialog";
import { expandVariables, parameterKeys, parseCommand, resolvePath, substitute } from "../editor/params";
import { type EditorHost, type RunMode, SqlEditor, type SqlEditorHandle } from "../editor/SqlEditor";
import { TERMINAL_ROWS, type TerminalBlock, TerminalView, blockText } from "../editor/terminal";
import { VariablesPanel } from "../editor/VariablesPanel";
import { defineSetting, getSetting, useSetting } from "../settings";
import { type Sort, viewerSql } from "../sqlNames";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { basename } from "../workspace";
import { type CheckedStatement, type GuardRequest, check, elapsed, transactionWarning } from "../guards/guards";
import { GuardDialog } from "../guards/GuardDialog";
import { ExportDialog, type ExportSource } from "./ExportDialog";
import { HealthBadge, useSessionHealth } from "./health";
import { HistoryPanel } from "./HistoryPanel";
import { useToolbar } from "./toolbar";
import { isSlow, slowQuerySeconds } from "../querylog/slow";
import { ask } from "../ui/ask";
import { Icon } from "../ui/Icon";
import { scrollSideways } from "../ui/scroll";
import { notifyRunEnded } from "../shell/notify";
import { type Result, ResultPanel, type ResultStatus, describe, fetchAll, fmt, formatMs } from "./ResultPanel";
import { t, plural, msg } from "../i18n";

/** Auto-refresh intervals in seconds, `0` for off. */
const REFRESH_CHOICES: [number, string][] = [
  [0, msg("Off")],
  [2, "2 s"],
  [5, "5 s"],
  [10, "10 s"],
  [30, "30 s"],
  [60, "1 min"],
  [300, "5 min"],
];

/** Asked before a run or a new filter drops the edits of the data grid. */
const DISCARD_EDITS = msg("Discard the changes to the data that are not saved?");

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

/** `sql` as the viewer compares queries: trimmed, without a final `;`. */
const queryKey = (sql: string) => sql.trim().replace(/;$/, "");

/** The range of the statement that produced `result` in the editor's `text`, when it still holds it. */
function statementRange(result: Result, text: string) {
  if (result.offset < 0) return null;
  const to = result.offset + result.sql.length;
  return text.slice(result.offset, to) === result.sql ? { from: result.offset, to } : null;
}

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
  /** Data viewer: server-side sort. */
  sort?: Sort | null;
  /** Values of `${name}` and `:name`, kept with the tab. */
  variables?: Record<string, string>;
  /** Results as text, like a console, instead of the grid. */
  terminal?: boolean;
}

/** The side panel next to the editor. */
type SidePanel = "history" | "variables" | "outline";

/** How many files `@include` may nest. */
const INCLUDE_DEPTH = 16;

const clientError = (message: string, kind = "other"): CommandError => ({
  kind,
  message,
  code: null,
  detail: null,
  hint: null,
  position: null,
});

declare module "../tabs/registry" {
  interface TabKinds {
    query: QueryTabModel;
  }
}

type Props = TabProps<QueryTabModel>;

interface Planned {
  sql: string;
  /** Where `sql` starts in the editor; -1 for statements of an `@include`d file. */
  offset: number;
  readOnly: boolean;
  /** Its statements and what they do, for the checks before running. */
  checks: CheckedStatement[];
  kind: StatementSpan["kind"];
  /** Placeholders, relative to `sql`. */
  params: ParameterSpan[];
  /** How many `@include`s deep it comes from. */
  depth: number;
}

/** What `runOne` learned about the result, for terminal mode. */
interface Seen {
  jobId: number | null;
  columns: ColumnInfo[] | null;
  rows: number;
}

/** The statements of `text`, from its analysis, at `offset` in the editor. */
function plannedOf(text: string, spans: StatementSpan[], offset: number, depth = 0): Planned[] {
  return spans.map((s) => {
    const sql = text.slice(s.start, s.end);
    return {
      sql,
      offset: offset < 0 ? -1 : offset + s.start,
      readOnly: s.read_only,
      checks: [{ sql, span: s }],
      kind: s.kind,
      params: s.parameters.map((p) => ({ ...p, start: p.start - s.start, end: p.end - s.start })),
      depth,
    };
  });
}

let nextResultId = 1;

/** A new result tab for `sql`, before it has a job. */
function newResult(sql: string, offset: number, status: ResultStatus, limit: number | null): Result {
  return { id: nextResultId++, sql, offset, jobId: null, columns: null, rows: 0, fetching: false, status, limit };
}

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
  currentSession,
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
  const slowSeconds = useSetting(slowQuerySeconds);
  const warned = useRef(false);
  const [count, setCount] = useState<string | null>(null);
  const [exportOpen, setExportOpen] = useState(false);
  const [exporting, setExporting] = useState<ExportProgress | null>(null);
  const [results, setResults] = useState<Result[]>([]);
  const [activeResult, setActiveResult] = useState(0);
  const [running, setRunning] = useState(false);
  // Whether the shown result's statement is in the editor and a read-only query the header
  // sort can rewrite with certainty.
  const shown = results[activeResult];
  useEffect(() => {
    let live = true;
    setSortable(false);
    if (!tab.table || shown?.status.kind !== "rows" || !statementRange(shown, tab.sql)) return;
    api.sqlSortable(shown.sql, connection.driver).then(
      (ok) => live && setSortable(ok),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [tab.table, shown?.id, shown?.sql, shown?.offset, shown?.status.kind, tab.sql, connection.driver]);
  /** The run going on was started by auto-refresh. */
  const autoRun = useRef(false);
  /** Texts of the data viewer's own queries: its first one and each sort's, to tell them from other queries. */
  const viewerQueries = useRef(new Set([queryKey(tab.sql)]));
  /** The data viewer's shown result is a query the header sort and quick filters can rewrite. */
  const [sortable, setSortable] = useState(false);
  const [blocked, setBlocked] = useState<RunMode | null>(null);
  const [panel, setPanel] = useState<SidePanel | null>(null);
  const togglePanel = (p: SidePanel) => setPanel((open) => (open === p ? null : p));
  /** A statement waiting for its parameters' values; `null` resolves as cancelled. */
  const [asking, setAsking] = useState<{
    sql: string;
    keys: string[];
    raw: Set<string>;
    initial: Record<string, string>;
    resolve: (values: Record<string, string> | null) => void;
  } | null>(null);
  const [terminalLog, setTerminalLog] = useState<TerminalBlock[]>([]);
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
  /** Parameter values answered in the last run, reused when it runs again. */
  const lastAnswers = useRef<Record<string, string>>({});
  /** The script running in the command-line client, which Cancel stops. */
  const cliRun = useRef<number | null>(null);
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
    if (!asksPassword(connection)) onNeedCatalog(false);
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
    const statements = analysis.statements;
    const all = plannedOf(scope.text, statements, scope.offset);
    // A selection runs as typed, even if it holds several statements, unless it has client
    // commands: those run one by one, as a script.
    if (mode === "script" || (from !== to && statements.some((s) => s.kind === "command"))) return all;
    if (from !== to) {
      if (!scope.text.trim()) return [];
      return [
        {
          sql: scope.text,
          offset: from,
          readOnly: statements.every((s) => s.read_only),
          checks: all.flatMap((p) => p.checks),
          kind: statements[0]?.kind ?? "query",
          params: statements.flatMap((s) => s.parameters),
          depth: 0,
        },
      ];
    }
    const at = analysis.at_cursor;
    return at === null || !all[at] ? [] : [all[at]];
  }

  /** Runs one statement and resolves with its first terminal status, once its job is known
   *  too (a fast statement finishes before `runQuery` returns the job); `seen` gets its job,
   *  columns and rows as they arrive. */
  function runOne(sessionId: number, result: Result, token: number, history: boolean, seen?: Seen): Promise<ResultStatus> {
    const limited = result.limit !== null;
    return new Promise((resolve) => {
      let final: ResultStatus | null = null;
      let jobKnown = false;
      const finish = () => {
        if (final && jobKnown) resolve(final);
      };
      const settle = (status: ResultStatus) => {
        if (!final && isTerminal(status, limited)) {
          final = status;
          finish();
        }
      };
      const onEvent = (event: QueryEvent) => {
        if (token !== runToken.current) return settle({ kind: "skipped" });
        let status: ResultStatus | null = null;
        switch (event.type) {
          case "started":
            status = { kind: "rows", finished: false, complete: true, elapsedMs: event.elapsed_ms };
            if (seen) seen.columns = event.columns;
            update(result.id, { columns: event.columns, fetching: true, status });
            break;
          case "progress":
            if (seen) seen.rows = event.rows;
            update(result.id, { rows: event.rows, fetching: event.fetching });
            break;
          case "finished":
            if (seen) seen.rows = event.rows;
            status = {
              kind: "rows",
              finished: true,
              complete: event.complete,
              stopped: event.stopped,
              elapsedMs: event.elapsed_ms,
            };
            update(result.id, { rows: event.rows, fetching: false, status });
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
          if (seen) seen.jobId = jobId;
          update(result.id, { jobId });
          jobKnown = true;
          finish();
        },
        (e: unknown) => {
          const status: ResultStatus = { kind: "error", error: toCommandError(e) };
          update(result.id, { status });
          jobKnown = true;
          settle(status);
        },
      );
    });
  }

  function showError(planned: Planned, status: ResultStatus) {
    if (status.kind !== "error" || status.error.kind === "cancelled" || planned.offset < 0) return;
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

  /** Asks the values of a statement's parameters that this run has not set yet; `null`
   *  when the user cancels. `defined` holds the names set so far in the run. */
  async function fillParams(
    p: Planned,
    variables: Record<string, string>,
    defined: Set<string>,
  ): Promise<Record<string, string> | null> {
    const keys = parameterKeys(p.params);
    const raw = new Set(p.params.filter((s) => s.kind === "variable").map((s) => s.name));
    const ask = [...new Set(keys)].filter((k) => (raw.has(k) ? !(k in variables) : !defined.has(k)));
    if (ask.length === 0) return variables;
    const answer = await new Promise<Record<string, string> | null>((resolve) =>
      setAsking({ sql: p.sql, keys: ask, raw, initial: variables, resolve }),
    );
    setAsking(null);
    if (!answer) return null;
    Object.assign(lastAnswers.current, answer);
    for (const k of ask) if (!k.startsWith("?")) defined.add(k);
    return { ...variables, ...answer };
  }

  /** Exports the rows of `sql` to `path` instead of showing them (`@export`). */
  function exportTo(sessionId: number, sql: string, path: string, format: ExportFormat): Promise<ResultStatus> {
    return new Promise((resolve) => {
      // SQL statements insert into a table named after the file.
      const table = path.split(/[\\/]/).pop()!.replace(/\.\w+$/, "");
      const options: ExportOptions = { format, delimiter: ",", header: true, table, driver: connection.driver };
      api
        .exportQuery(sessionId, sql, path, options, (event) => {
          if (event.type === "done") {
            const rows = fmt.format(event.rows);
            const text = t("Exported {rows} rows to {path} in {time}", { rows, path, time: formatMs(event.elapsed_ms) });
            resolve({ kind: "message", summary: t("Exported {rows} rows", { rows }), text });
          } else if (event.type === "failed") {
            resolve({ kind: "error", error: event.error });
          } else if (event.type === "cancelled") {
            resolve({ kind: "error", error: clientError(t("Export cancelled"), "cancelled") });
          }
        })
        .catch((e: unknown) => resolve({ kind: "error", error: toCommandError(e) }));
    });
  }

  /** Adds a finished statement's text to the terminal-mode output. */
  async function logTerminal(id: number, sql: string, status: ResultStatus, seen?: Seen) {
    const text = await blockText(status, seen?.jobId ?? null, seen?.columns ?? null, seen?.rows ?? 0).catch(
      (e: unknown) => toCommandError(e).message,
    );
    const failed = status.kind === "error" || (status.kind === "message" && !!status.failed);
    setTerminalLog((log) => [...log, { id, sql, text, failed }]);
  }

  /** Shows the checks before running `planned` (UPDATE without WHERE, production
   *  changes); false when the user declines. */
  async function confirmed(planned: Planned[]): Promise<boolean> {
    const request = await check(connection, planned.flatMap((p) => p.checks), sideSession);
    return !request || new Promise<boolean>((resolve) => setGuard({ request, resolve }));
  }

  /** Runs what `mode` selects in the editor, or `again` (the statements of the last run).
   *  An `auto` refresh stays out of the query history, keeps the result tab shown and the
   *  editor as it is, and stops refreshing when a statement fails. */
  async function run(mode: RunMode, force = false, again?: Planned[], auto = false) {
    const history = !auto;
    if (running && !force) return;
    if (unsaved.current && !(await ask(t(DISCARD_EDITS), { confirm: t("Discard"), danger: true }))) return;
    const append = mode === "append";
    let planned: Planned[];
    try {
      planned = again ?? (await plan(append ? "statement" : mode));
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
    if (!again && !(await confirmed(planned))) return;
    const sessionId = await ensureSession();
    if (sessionId === null) return;
    onNeedCatalog(false);

    const token = ++runToken.current;
    if (!append) closeJobs();
    const script = mode === "script" && planned.length > 1;
    const limit = tab.terminal ? TERMINAL_ROWS : script ? getSetting(scriptRowLimit) : null;
    const resultOf = (p: Planned, first: boolean) =>
      newResult(p.sql, p.offset, { kind: first ? "running" : "pending" }, limit);
    // `queue` grows when an `@include` adds a file's statements after itself.
    const queue = [...planned];
    const own = queue.map((p, i) => resultOf(p, i === 0));
    const base = append ? results.length : 0;
    setResults((all) => (append ? [...all, ...own] : own));
    if (!auto) setActiveResult(base);
    autoRun.current = auto;
    setRunning(true);
    editor.current!.setError(null);

    // A run again after saving edited rows reuses the values answered last time.
    if (!again) lastAnswers.current = {};
    const variables = { ...(tab.variables ?? {}), ...lastAnswers.current };
    const defined = new Set<string>(again ? Object.keys(variables) : []);
    const keepVariables = () =>
      onChange({ variables: Object.fromEntries(Object.entries(variables).filter(([k]) => !k.startsWith("?"))) });
    // Set by `@export`, taken by the next statement.
    const pending: { export: { path: string; format: ExportFormat } | null } = { export: null };

    /** Runs a client command; an `@include` adds the file's statements after `i`. */
    async function command(p: Planned, i: number): Promise<ResultStatus> {
      const c = parseCommand(expandVariables(p.sql, variables));
      switch (c.name) {
        case "set":
          variables[c.variable] = c.value;
          defined.add(c.variable);
          keepVariables();
          return { kind: "message", summary: `${c.variable} = ${c.value}`, text: `${c.variable} = ${c.value}` };
        case "echo":
          return { kind: "message", summary: c.text.slice(0, 60) || t("(empty)"), text: c.text };
        case "export": {
          const path = resolvePath(c.path, tab.filePath);
          if (!path) return { kind: "error", error: clientError(t("Save the script first, or give @export an absolute path.")) };
          pending.export = { path, format: c.format };
          return { kind: "message", summary: t("Next rows to {path}", { path: c.path }), text: t("The next statement's rows go to {path}.", { path }) };
        }
        case "error":
          return { kind: "error", error: clientError(c.message) };
        case "include": {
          const path = resolvePath(c.path, tab.filePath);
          if (!path) return { kind: "error", error: clientError(t("Save the script first, or give @include an absolute path.")) };
          if (p.depth >= INCLUDE_DEPTH) {
            return { kind: "error", error: clientError(t("@include nests more than {n} files.", { n: INCLUDE_DEPTH })) };
          }
          try {
            const text = await api.readTextFile(path);
            const spans = (await api.analyzeSql(text, connection.driver, 0)).statements;
            const added = plannedOf(text, spans, -1, p.depth + 1);
            // The file's statements get the checks the editor's had before the run.
            if (connection.read_only && added.some((a) => !a.readOnly)) {
              return { kind: "error", error: clientError(t("{name} is read-only and {path} may modify data.", { name: connection.name, path })) };
            }
            if (!(await confirmed(added))) return { kind: "error", error: clientError(t("Cancelled"), "cancelled") };
            const addedResults = added.map((a) => resultOf(a, false));
            queue.splice(i + 1, 0, ...added);
            own.splice(i + 1, 0, ...addedResults);
            const after = own[i]!.id;
            setResults((all) => {
              const at = all.findIndex((r) => r.id === after);
              return [...all.slice(0, at + 1), ...addedResults, ...all.slice(at + 1)];
            });
            const n = added.length;
            return {
              kind: "message",
              summary: plural(n, "Included {n} statement", "Included {n} statements"),
              text: plural(n, "Included {path}: {n} statement", "Included {path}: {n} statements", { path }),
            };
          } catch (e) {
            return { kind: "error", error: toCommandError(e) };
          }
        }
      }
    }

    // Afterwards show the failed statement, else the last result set, else the last result.
    let failed = -1;
    let lastRows = -1;
    for (let i = 0; i < queue.length; i++) {
      const result = own[i]!;
      const p = queue[i]!;
      if (token !== runToken.current) return;
      update(result.id, { status: { kind: "running" } });
      if (!auto) {
        setActiveResult(base + i);
        if (p.offset >= 0) editor.current!.flash(p.offset, p.offset + p.sql.length);
      }
      const seen: Seen = { jobId: null, columns: null, rows: 0 };
      let status: ResultStatus;
      let sql = p.sql;
      if (p.kind === "command") {
        status = await command(p, i);
      } else {
        const values = p.params.length ? await fillParams(p, variables, defined) : variables;
        if (!values) {
          status = { kind: "error", error: clientError(t("Cancelled"), "cancelled") };
        } else {
          if (p.params.length) {
            sql = substitute(p.sql, p.params, values, connection.driver);
            Object.assign(variables, values);
            keepVariables();
          }
          const ran = { ...result, sql };
          update(result.id, { sql });
          const target = pending.export;
          pending.export = null;
          if (target) {
            status = await exportTo(sessionId, sql, target.path, target.format);
          } else {
            status = await runOne(sessionId, ran, token, history, seen);
            // A dropped connection fails before the statement reaches the server; the
            // second attempt runs on a reopened connection.
            if (status.kind === "error" && status.error.kind === "closed" && token === runToken.current) {
              update(result.id, { status: { kind: "running" } });
              status = await runOne(sessionId, ran, token, history, seen);
            }
          }
        }
      }
      if (token !== runToken.current) return;
      // runOne reports the statuses of a job; commands, exports and cancelled ones are set here.
      if (seen.jobId === null) update(result.id, { status });
      if (tab.terminal) await logTerminal(result.id, sql, status, seen);
      if (status.kind === "rows") lastRows = i;
      if (status.kind === "error") {
        failed = i;
        // The editor may have changed since the statements were planned: no marker there.
        if (!auto) showError(p, status);
        else onError(t("Auto-refresh stopped: {message}", { message: status.error.message }));
        const skipped = new Set(own.slice(i + 1).map((r) => r.id));
        setResults((all) => all.map((r) => (skipped.has(r.id) ? { ...r, status: { kind: "skipped" } } : r)));
        break;
      }
    }
    if (auto && failed >= 0) setRefreshEvery(null);
    if (!auto || failed >= 0) setActiveResult(base + (failed >= 0 ? failed : lastRows >= 0 ? lastRows : own.length - 1));
    setRunning(false);
    currentJob.current = null;
    if (queue.some((p) => DDL.test(p.sql))) {
      onNeedCatalog(true);
      details.current = null;
      sources.current.clear();
    }
    setHistoryVersion((v) => v + 1);
    await refreshTx(sessionId);
  }

  /** Runs the selection, or the whole editor, with the database's command-line client. */
  async function runCli() {
    if (running) return;
    const ed = editor.current!;
    const { from, to } = ed.getSelection();
    const script = from !== to ? ed.getText().slice(from, to) : ed.getText();
    if (!script.trim()) return;
    const planned = plannedOf(script, (await api.analyzeSql(script, connection.driver, 0)).statements, 0);
    if (
      connection.read_only &&
      planned.some((p) => !p.readOnly) &&
      !(await ask(t("{name} is read-only and this may modify data. Run anyway?", { name: connection.name }), { confirm: t("Run") }))
    ) {
      return;
    }
    if (!(await confirmed(planned))) return;
    const sessionId = await ensureSession();
    if (sessionId === null) return;
    const client = cliName(connection.driver);
    const result = newResult(script, from !== to ? from : 0, { kind: "running" }, null);
    runToken.current++;
    closeJobs();
    setResults([result]);
    setActiveResult(0);
    autoRun.current = false;
    setRunning(true);
    let status: ResultStatus;
    try {
      cliRun.current = result.id;
      const out = await api.runCli(sessionId, result.id, script);
      const failed = out.status !== 0;
      status = {
        kind: "message",
        summary: failed
          ? t("{client} failed (exit {code}) in {time}", { client: client ?? "", code: out.status ?? "signal", time: formatMs(out.elapsed_ms) })
          : t("{client} finished in {time}", { client: client ?? "", time: formatMs(out.elapsed_ms) }),
        text: out.output || t("(no output)"),
        failed,
      };
    } catch (e) {
      status = { kind: "error", error: toCommandError(e) };
    }
    cliRun.current = null;
    update(result.id, { status });
    if (tab.terminal) await logTerminal(result.id, script, status);
    setRunning(false);
    onNeedCatalog(true);
    await refreshTx(sessionId);
  }

  /** Opens the table, view or routine a name in the SQL refers to (Ctrl+click). */
  async function openObject(schema: string | null, name: string) {
    const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
    try {
      const id = await sideId();
      const schemas = await api.listSchemas(id);
      const candidates = schema
        ? schemas.filter((s) => same(s.name, schema))
        : [...schemas.filter((s) => s.current), ...schemas.filter((s) => !s.current && !s.system)];
      for (const s of candidates) {
        const objects = (await api.listObjects(id, s.name)).filter((o) => o.kind !== "trigger");
        const base = (o: { name: string }) => o.name.replace(/\(.*$/, "");
        const object = objects.find((o) => base(o) === name) ?? objects.find((o) => same(base(o), name));
        if (object) {
          workbench().openTab("object", connection.id, {
            database: tab.database,
            schema: s.name,
            currentSchema: s.current,
            name: object.name,
            objectKind: object.kind,
            parent: object.parent,
            title: object.name,
          });
          return;
        }
      }
      onError(t("No table, view or routine named {name}", { name: schema ? `${schema}.${name}` : name }));
    } catch (e) {
      onError(toCommandError(e).message);
    }
  }

  /** Opens the plan of the selection or the statement under the cursor in a new tab. */
  async function explain(analyze: boolean) {
    let p: Planned | undefined;
    try {
      [p] = await plan("statement");
    } catch (e) {
      onError(toCommandError(e).message);
      return;
    }
    if (!p || p.kind === "command") return;
    let sql = p.sql;
    if (p.params.length) {
      const values = await fillParams(p, { ...(tab.variables ?? {}), ...lastAnswers.current }, new Set());
      if (!values) return;
      sql = substitute(p.sql, p.params, values, connection.driver);
    }
    // Analyze runs the statement: the checks of a run apply.
    if (analyze) {
      if (connection.read_only && !p.readOnly) {
        onError(t("{name} is read-only and this statement may modify data.", { name: connection.name }));
        return;
      }
      if (!(await confirmed([p]))) return;
    }
    workbench().openTab("explain", connection.id, { database: tab.database, sql, analyze });
  }

  const host: EditorHost = {
    openObject: (schema, name) => void openObject(schema, name),
    runCli: cliName(connection.driver) ? () => void runCli() : undefined,
    runAppend: () => void run("append"),
    explain: isMongo(connection.driver) ? undefined : (analyze) => void explain(analyze),
  };

  /** Closes a result tab and frees its rows. */
  function closeResult(id: number) {
    const i = results.findIndex((r) => r.id === id);
    const r = results[i];
    // The run in progress places its results by position.
    if (!r || running) return;
    if (r.jobId !== null) {
      void api.closeQuery(r.jobId);
      jobs.current = jobs.current.filter((j) => j !== r.jobId);
    }
    setResults((all) => all.filter((x) => x.id !== id));
    setActiveResult((a) => Math.max(0, a > i || a === results.length - 1 ? a - 1 : a));
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

  /**
   * Data viewer: rewrites the statement that produced `result` in the editor (only that
   * statement, the rest of the text stays) and runs it. `false` when the editor no longer holds
   * the statement or the rewrite cannot place the change with certainty.
   */
  async function rewriteStatement(result: Result, rewrite: (sql: string) => Promise<string | null>): Promise<boolean> {
    const ed = editor.current!;
    const text = ed.getText();
    const range = statementRange(result, text);
    let next: string | null = null;
    try {
      next = range ? await rewrite(result.sql) : null;
    } catch (e) {
      onError(toCommandError(e).message);
      return false;
    }
    if (!range || next === null) {
      onError(t("This query cannot be changed from the grid: edit it in the editor."));
      return false;
    }
    viewerQueries.current.add(queryKey(next));
    ed.setText(text.slice(0, range.from) + next + text.slice(range.to));
    // The cursor inside the statement, so that it is the one that runs.
    const inside = range.from + Math.floor(next.length / 2);
    ed.select(inside, inside);
    void run("statement");
    return true;
  }

  /** Goes on once edited rows are saved or discarded. */
  function afterDiscard(go: () => void) {
    if (!unsaved.current) return go();
    void ask(t(DISCARD_EDITS), { confirm: t("Discard"), danger: true }).then((yes) => {
      if (!yes) return;
      unsaved.current = false;
      go();
    });
  }

  /** Data viewer: sorts the shown result's statement and runs it. */
  function applyViewer(sort: Sort | null) {
    const result = results[activeResult];
    if (!tab.table || !result) return;
    afterDiscard(() => {
      const column = sort?.column ?? null;
      void rewriteStatement(result, (sql) => api.sqlOrderBy(sql, connection.driver, column, sort?.desc ?? false)).then((done) => {
        if (!done) return;
        onChange({ sort });
        setCount(null);
      });
    });
  }

  /** Quick filter of the grid: adds `condition` to the WHERE of the shown result's statement. */
  function addCondition(condition: string) {
    const result = results[activeResult];
    if (!result) return;
    afterDiscard(() => {
      void rewriteStatement(result, (sql) => api.sqlAddCondition(sql, connection.driver, condition)).then(
        (done) => done && setCount(null),
      );
    });
  }

  /** Whether `sql` is one of the viewer's own queries: it reads exactly the viewer's table. */
  function isViewerQuery(sql: string) {
    return viewerQueries.current.has(queryKey(sql)) || sql === viewerSql(connection.driver, tab.table ?? "");
  }

  /** Header clicks cycle a column through ascending, descending and unsorted. */
  function sortBy(column: string) {
    const sort = tab.sort ?? null;
    if (sort?.column !== column) applyViewer({ column, desc: false });
    else if (!sort.desc) applyViewer({ column, desc: true });
    else applyViewer(null);
  }

  async function countRows() {
    if (!tab.table) return;
    setCount(t("counting…"));
    const sessionId = await sideSession();
    if (sessionId === null) return setCount(null);
    try {
      // The statement of the shown result, else the editor's text.
      const query = await api.sqlCountQuery(results[activeResult]?.sql ?? editor.current!.getText(), connection.driver);
      if (query === null) {
        setCount(null);
        return onError(t("This query cannot be counted."));
      }
      const value = await api.queryValue(sessionId, query);
      setCount(value === null ? null : plural(Number(value), "{n} row", "{n} rows"));
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
        finish(t("Exported {rows} rows to {path} in {time}", { rows: fmt.format(rows), path: file, time: formatMs(Math.round(performance.now() - started)) }));
      } catch (e) {
        finish(t("Export failed: {message}", { message: toCommandError(e).message }), true);
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
            finish(t("Exported {rows} rows to {path} in {time}", { rows: fmt.format(event.rows), path: file, time: formatMs(event.elapsed_ms) }));
            break;
          case "failed":
            finish(t("Export failed: {message}", { message: event.error.message }), true);
            break;
          case "cancelled":
            finish(t("Export cancelled"));
            break;
        }
      });
      setExporting((e) => e && e.done === null ? { ...e, exportId } : e);
    } catch (e) {
      finish(t("Export failed: {message}", { message: toCommandError(e).message }), true);
    }
  }

  async function cancel() {
    const job = currentJob.current;
    const cli = cliRun.current;
    try {
      if (cli !== null) await api.cancelCli(cli);
      else if (job !== null) await api.cancelQuery(job);
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
    if (id === null) throw new Error(t("no connection"));
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
    // The data editor writes SQL: MongoDB results stay read-only.
    if (isMongo(connection.driver)) return null;
    const ref = tab.tableRef;
    const viewer =
      !!tab.table &&
      !!ref &&
      isViewerQuery(result.sql);
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

  /** The viewer's table, for the notes in the grid header; none for other results. */
  function notesScope(result: Result): NotesScope | null {
    const ref = tab.tableRef;
    if (!(tab.table && ref) || isMongo(connection.driver)) return null;
    if (!isViewerQuery(result.sql)) return null;
    return { connectionId: connection.id, database: tab.database, schema: ref.schema, table: ref.name };
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
    onError(t("A transaction on {name} has been open for {n} minutes: commit or roll back.", { name: connection.name, n: warnMinutes }));
  }, [txLong]);

  // Auto-refresh: the last statements again, if they only read, while the tab is shown and
  // nothing runs, asks or waits to be saved. The timer calls the latest render's `refresh`.
  const refresh = useRef(() => {});
  refresh.current = () => {
    const again = lastPlanned.current;
    if (running || guard || unsaved.current || again.length === 0) return;
    if (again.some((p) => !p.readOnly)) {
      setRefreshEvery(null);
      onError(t("Auto-refresh stopped: the last statements change data."));
      return;
    }
    void run("statement", false, again, true);
  };
  useEffect(() => {
    if (refreshEvery === null || !active) return;
    const timer = setInterval(() => refresh.current(), refreshEvery * 1000);
    return () => clearInterval(timer);
  }, [refreshEvery, active]);

  // A long run that ends while the window is in the background is notified, unless
  // auto-refresh started it.
  const runStart = useRef(0);
  useEffect(() => {
    if (running) runStart.current = Date.now();
    else if (runStart.current && !autoRun.current) {
      const failed = results.some((r) => r.status.kind === "error" || (r.status.kind === "message" && r.status.failed));
      void notifyRunEnded(runStart.current, connection.name, failed);
    }
  }, [running]);

  const health = useSessionHealth(currentSession, active && !running && !guard);
  const toolbar = useToolbar();
  // ClickHouse and MongoDB have no transactions (`capabilities.transactions` is false): no
  // commit mode.
  const showTx = toolbar.visible.transactions && connection.driver !== "clickhouse" && !isMongo(connection.driver);
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
            title={t("Run statement (Ctrl+Enter)")}
            aria-label={t("Run")}
          >
            <Icon name="run" />
            {t("Run")}
          </button>
          {toolbar.visible.script && (
            <button onClick={() => void run("script")} title={t("Run script (Alt+X, Ctrl+Shift+Enter)")} aria-label={t("Script")}>
              {toolbar.label("script", t("Script"))}
            </button>
          )}
          {toolbar.visible.cancel && (
            <button
              onClick={() => void cancel()}
              disabled={!running}
              title={t("Cancel the running statement")}
              aria-label={t("Cancel")}
            >
              {toolbar.label("cancel", t("Cancel"))}
            </button>
          )}
          <ConnectionChip connection={connection} database={tab.database ?? null} schema={catalog?.default_schema ?? null} />
          {showTx && <span className="divider" />}
          {showTx && (
          <button
            className={manual ? "toggled" : ""}
            onClick={() => void toggleAutocommit()}
            disabled={running || !!tx?.open}
            aria-label={t(manual ? "Manual commit" : "Auto-commit")}
            title={t(
              tx?.open
                ? "Commit or roll back before switching"
                : manual
                  ? "Manual commit: changes wait for Commit"
                  : "Auto-commit: every statement commits on its own",
            )}
          >
            {toolbar.label(manual ? "manual-commit" : "autocommit", t(manual ? "Manual commit" : "Auto-commit"))}
          </button>
          )}
          {showTx && manual && (
            <>
              <button
                onClick={() => void endTransaction(true)}
                disabled={running || !tx.open}
                title={t("Commit")}
                aria-label={t("Commit")}
              >
                {toolbar.label("commit", t("Commit"))}
              </button>
              <button
                onClick={() => void endTransaction(false)}
                disabled={running || !tx.open}
                title={t("Rollback")}
                aria-label={t("Rollback")}
              >
                {toolbar.label("rollback", t("Rollback"))}
              </button>
            </>
          )}
          {tx?.open && (
            <span
              className={`tx-open${txLong ? " long" : ""}`}
              title={t("A transaction is open: changes are not visible to others yet, and locks are held")}
            >
              ● {t("Transaction open {time}", { time: elapsed(txAge) })}
            </span>
          )}
          {toolbar.visible.files && (
            <>
              <span className="divider" />
              <button onClick={() => void openFile()} title={t("Open script (Ctrl+O)")} aria-label={t("Open")}>
                {toolbar.label("open", t("Open"))}
              </button>
              <button onClick={() => void saveFile()} title={t("Save script (Ctrl+S)")} aria-label={t("Save")}>
                {toolbar.label("save", t("Save"))}
                {dirty ? " •" : ""}
              </button>
            </>
          )}
          {toolbar.visible.refresh && (
            <label
              className={`auto-refresh${refreshEvery !== null ? " toggled" : ""}`}
              title={t("Run the last statements again every few seconds, if they only read data")}
            >
              <Icon name="refresh" />
              <select
                aria-label={t("Auto-refresh")}
                value={refreshEvery ?? 0}
                onChange={(e) => setRefreshEvery(Number(e.target.value) || null)}
              >
                {REFRESH_CHOICES.map(([seconds, label]) => (
                  <option key={seconds} value={seconds}>
                    {t(label)}
                  </option>
                ))}
              </select>
            </label>
          )}
          <span className="spacer" />
          {tab.filePath && <span className="muted small">{basename(tab.filePath)}</span>}
          {toolbar.visible.panels && (
            <>
              <button
                className={tab.terminal ? "toggled" : ""}
                onClick={() => onChange({ terminal: !tab.terminal })}
                title={t("Terminal mode: results as text, kept across runs")}
                aria-label={t("Terminal")}
              >
                {toolbar.label("terminal", t("Terminal"))}
              </button>
              <button
                className={panel === "variables" ? "toggled" : ""}
                onClick={() => togglePanel("variables")}
                title={t("Variables of ${name} and :name")}
                aria-label={t("Variables")}
              >
                {toolbar.label("variables", t("Variables"))}
              </button>
              <button
                className={panel === "outline" ? "toggled" : ""}
                onClick={() => togglePanel("outline")}
                title={t("Statements of the editor")}
                aria-label={t("Outline")}
              >
                {toolbar.label("outline", t("Outline"))}
              </button>
            </>
          )}
          {toolbar.visible.history && (
            <button className={panel === "history" ? "toggled" : ""} onClick={() => togglePanel("history")} title={t("History")} aria-label={t("History")}>
              {toolbar.label("history", t("History"))}
            </button>
          )}
        </div>
        {tab.table && (
          <div className="viewer-bar">
            <span className="spacer" />
            {count && <span className="muted small">{count}</span>}
            <button
              type="button"
              className="square"
              onClick={() => void countRows()}
              aria-label={t("Count rows")}
              title={t(isMongo(connection.driver) ? "Count rows: runs countDocuments on the collection" : "Count rows: counts the rows of the query in the editor")}
            >
              <Icon name="sequence" />
            </button>
          </div>
        )}
        <SqlEditor
          ref={editor}
          initialValue={tab.sql}
          driver={connection.driver}
          catalog={catalog}
          connectionId={connection.id}
          database={tab.database}
          production={isProduction(connection)}
          host={host}
          onChange={(sql) => onChange({ sql })}
          onRun={(mode) => void run(mode)}
          onSave={() => void saveFile()}
          onOpen={() => void openFile()}
        />
        <div className="splitter" onMouseDown={startResize} />
        <div className="results">
          {blocked !== null && (
            <div className="notice warn">
              {t("{name} is read-only and this may modify data.", { name: connection.name })}
              <button onClick={() => void run(blocked, true)}>{t("Run anyway")}</button>
            </div>
          )}
          {tab.terminal && <TerminalView blocks={terminalLog} onClear={() => setTerminalLog([])} />}
          {!tab.terminal && results.length > 1 && (
            <div className="result-tabs" onWheel={scrollSideways}>
              {results.map((r, i) => (
                <button
                  key={r.id}
                  className={`result-tab ${r.status.kind}${i === activeResult ? " active" : ""}`}
                  onClick={() => setActiveResult(i)}
                  onAuxClick={(e) => e.button === 1 && closeResult(r.id)}
                  title={r.sql}
                >
                  {i + 1}. {describe(r)}
                  <span
                    className="result-tab-close"
                    role="button"
                    aria-label={t("Close result")}
                    title={t("Close result")}
                    onClick={(e) => {
                      e.stopPropagation();
                      closeResult(r.id);
                    }}
                  >
                    ×
                  </span>
                </button>
              ))}
            </div>
          )}
          {!tab.terminal && current && (
            <ResultPanel
              key={current.id}
              result={current}
              sort={tab.table ? (tab.sort ?? null) : null}
              onHeaderClick={tab.table && sortable && !running ? sortBy : undefined}
              headerHint={
                tab.table && !sortable && current.status.kind === "rows"
                  ? t("This query cannot be sorted from the header: it is not one read-only query that an ORDER BY can be added to.")
                  : undefined
              }
              source={{
                connection,
                sql: current.sql,
                jobId: current.jobId,
                table: tab.table ?? null,
                viewer: tab.table && sortable ? { addCondition } : undefined,
              }}
              edit={editTarget(current)}
              notes={notesScope(current)}
            />
          )}
        </div>
        <div className="statusbar" role="status">
          {current && (
            <>
              <span>{describe(current)}</span>
              {(current.status.kind === "rows" || current.status.kind === "affected") && (
                <span className="muted">
                  {t(current.status.kind === "rows" && !current.status.finished ? "first rows in {time}" : "took {time}", {
                    time: formatMs(current.status.elapsedMs),
                  })}
                </span>
              )}
              {(current.status.kind === "rows" || current.status.kind === "affected") &&
                isSlow(current.status.elapsedMs, slowSeconds) && (
                  <span
                    className="slow-query"
                    title={t("Slower than {n} s (Settings). Explain (Ctrl+Shift+E) shows where the time goes.", { n: slowSeconds })}
                  >
                    {t("slow")}
                  </span>
                )}
              {current.status.kind === "rows" && !current.status.finished && (
                <button className="link" onClick={() => fetchAll(current)}>
                  {t("Fetch all")}
                </button>
              )}
              {rowsResult && !exportBusy && (
                <button className="link" onClick={() => setExportOpen(true)}>
                  {t("Export…")}
                </button>
              )}
            </>
          )}
          {exporting && (
            <span className={exporting.failed ? "error-text" : "muted"}>
              {exporting.done ?? t("Exporting… {rows} rows", { rows: fmt.format(exporting.rows) })}
              {exportBusy && exporting.exportId !== null && (
                <button className="link" onClick={() => void api.cancelExport(exporting.exportId!)}>
                  {t("Cancel")}
                </button>
              )}
              {!exportBusy && (
                <button className="link" onClick={() => setExporting(null)} title={t("Dismiss")} aria-label={t("Dismiss")}>
                  ×
                </button>
              )}
            </span>
          )}
          {undoScript && (
            <span className="muted">
              {t("Changes saved.")}
              <button
                className="link"
                onClick={() => workbench().openTab("query", connection.id, { sql: undoScript })}
                title={t("Opens the SQL that puts the rows back as they were")}
              >
                {t("Undo script")}
              </button>
              <button className="link" onClick={() => setUndoScript(null)} title={t("Dismiss")} aria-label={t("Dismiss")}>
                ×
              </button>
            </span>
          )}
          <span className="spacer" />
          <HealthBadge health={health} />
          <span className="muted">
            {t("Ctrl+Enter statement · Alt+X script")} · {connection.name}
          </span>
        </div>
      </div>
      {panel === "history" && (
        <HistoryPanel
          connectionId={connection.id}
          refresh={historyVersion}
          onInsert={(sql) => editor.current?.insert(sql)}
          onClose={() => setPanel(null)}
        />
      )}
      {panel === "variables" && (
        <VariablesPanel
          variables={tab.variables ?? {}}
          onChange={(variables) => onChange({ variables })}
          onClose={() => setPanel(null)}
        />
      )}
      {panel === "outline" && (
        <OutlinePanel
          text={tab.sql}
          driver={connection.driver}
          onSelect={(from, to) => editor.current?.select(from, to)}
          onClose={() => setPanel(null)}
        />
      )}
      {asking && (
        <ParamsDialog
          sql={asking.sql}
          keys={asking.keys}
          raw={asking.raw}
          initial={asking.initial}
          onRun={asking.resolve}
          onCancel={() => asking.resolve(null)}
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
          driver={connection.driver}
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
    <span className={`conn-chip${connection.connection_type ? ` ${connection.connection_type}` : ""}`} title={t("Statements run here")}>
      <span className="conn-chip-dot" />
      {parts.join(" · ")}
    </span>
  );
}
