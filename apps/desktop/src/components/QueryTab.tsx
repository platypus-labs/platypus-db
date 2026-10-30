import { useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { titleWithKeys } from "../actions";
import {
  api,
  toCommandError,
  type ColumnInfo,
  type CommandError,
  type ExportOptions,
  type FilterTarget,
  type ObjectMeta,
  type ParameterSpan,
  type StatementSpan,
  type SqlAnalysis,
  type QueryEvent,
  type SavedConnection,
  type SavedResult,
  type TableDetails,
  type TransactionState,
  isMongo,
  isNoSql,
  isRedis,
  keepsDataIn,
} from "../api";
import { asksPassword, isProduction } from "../connections";
import type { NotesScope } from "../grid/ResultGrid";
import type { EditSource, TableRef } from "../grid/edit/meta";
import type { TableEditTarget } from "../grid/edit/TableEditor";
import { cliName } from "../editor/actions";
import { OutlinePanel } from "../editor/OutlinePanel";
import { ParamsDialog } from "../editor/ParamsDialog";
import { type ExportTarget, expandVariables, parameterKeys, parseCommand, resolvePath, substitute } from "../editor/params";
import { originalOffset, utf16Offset } from "../editor/literals";
import { setsSchema, usedDatabase } from "../editor/context";
import { MultiRunDialog } from "../editor/MultiRunDialog";
import { type EditorHost, type RunMode, SqlEditor, type SqlEditorHandle } from "../editor/SqlEditor";
import { TERMINAL_ROWS, type TerminalBlock, TerminalView, blockText } from "../editor/terminal";
import { VariablesPanel } from "../editor/VariablesPanel";
import { defineSetting, getSetting, useSetting } from "../settings";
import { qualifiedName, viewerSql } from "../sqlNames";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { editsTables, openObjectTab, openTableEditor } from "../objects/open";
import { ResultSourceBar } from "../analysis/QueryWithSql";
import { AcrossBar } from "../analysis/CrossQueryDialog";
import type { AcrossOrigin } from "../analysis/acrossModel";
import { SavedResultBar } from "../analysis/SavedResults";
import { useResultSql } from "../analysis/ResultSql";
import { provideContext } from "../agents/context";
import { type TabParts, tabSnapshot } from "../agents/contextTab";
import { provideResults, resultsWithRows, type ResultSource } from "../analysis/resultHosts";
import { basename } from "../workspace";
import {
  type CheckedStatement,
  type GuardRequest,
  autoLimitSetting,
  check,
  elapsed,
  transactionWarning,
} from "../guards/guards";
import { GuardDialog } from "../guards/GuardDialog";
import { PendingDialog } from "../guards/PendingDialog";
import { PanelResizer, useStoredSize } from "../shell/PanelResizer";
import { TransactionLogDialog } from "../guards/TransactionLogDialog";
import { openRestore, snapshotBefore } from "../guards/SnapshotsDialog";
import { emptyLog, noteEnd, noteStatement } from "../guards/txLog";
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
import { ChartToggle } from "../analysis/ChartView";
import { chartOf, type SavedChart, withChart } from "../analysis/chartConfig";
import { runGate } from "./runGate";
import { RunMemory, rerunsUnasked } from "./rerun";
import { requestNavigatorRefresh } from "../navigatorEvents";
import {
  type ShownSort,
  type ViewerSort,
  clickSort,
  queryKey,
  sortOf,
  statementRange,
  viewerQueries,
  withSort,
} from "./viewerSort";
import { useCompare } from "../analysis/compare";
import { editMeta } from "../grid/edit/meta";
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
  /** Data viewer: the header sorts, each with the statement it was written into. */
  sorts?: ViewerSort[];
  /** Data viewer: its own query, the one it opened with or the last rewrite of it by the header
   *  sort or a quick filter. Its rows are the table's rows; another query typed in the tab is
   *  not the viewer's. */
  viewerQuery?: string | null;
  /** Values of `${name}` and `:name`, kept with the tab. */
  variables?: Record<string, string>;
  /** "Query with SQL": where the tables of this tab came from. Not persisted: the temporary
   *  connection does not outlive the app. */
  resultSource?: ResultSource;
  /** "Query across databases": the tables copied into this tab's temporary connection.
   *  Not persisted, like `resultSource`. */
  across?: AcrossOrigin;
  /** A saved result opened offline: its tables are the file's rows, not a result's. Not
   *  persisted, like the temporary connection. */
  savedResult?: SavedResult;
  /** Results as text, like a console, instead of the grid. */
  terminal?: boolean;
  /** Charts of the statements, each shown in place of the grid when chosen. */
  charts?: SavedChart[];
  /** Read-only for this tab only, as if the connection were; not persisted. */
  readOnly?: boolean;
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

/** The status of a statement the user stopped before it had a result. */
const cancelled = (): ResultStatus => ({ kind: "error", error: clientError(t("Cancelled"), "cancelled") });

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
  /** The file it comes from (an `@include`), against which its own `@include`s resolve;
   *  `null` for the editor's text. */
  file?: string | null;
  /** Runs this many times (T-SQL `GO 3`). */
  repeat?: number;
}

/** What `runOne` learned about the result, for terminal mode. */
interface Seen {
  jobId: number | null;
  columns: ColumnInfo[] | null;
  rows: number;
  /** The result its last status belongs to, when the statement had several. */
  resultId?: number;
}

/** How often the results of a run are drawn while it goes on. */
const RENDER_EVERY_MS = 60;

/** `all` with `added` placed right after the result `id` (at the end when it is gone). */
function withAfter(all: Result[], id: number, added: Result[]): Result[] {
  const at = all.findIndex((r) => r.id === id);
  return at < 0 ? [...all, ...added] : [...all.slice(0, at + 1), ...added, ...all.slice(at + 1)];
}

/** The statements of `text`, from its analysis, at `offset` in the editor. */
function plannedOf(text: string, spans: StatementSpan[], offset: number, depth = 0, file: string | null = null): Planned[] {
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
      file,
    };
  });
}

/** What a group of statements sent together does, for the checks and the run: a query only
 *  if each is one. */
function groupKind(group: Planned[]): StatementSpan["kind"] {
  if (group.length === 1) return group[0]!.kind;
  return group.find((p) => p.kind !== "query")?.kind ?? "other";
}

/** The statements of a script as they go to the server: in Transact-SQL a batch (up to `GO`)
 *  together, as one text, so that its variables live from one statement to the next; else
 *  each alone. `all` are `analysis`'s statements of `text`, planned at `offset`. */
function scriptOf(text: string, analysis: SqlAnalysis, all: Planned[], offset: number): Planned[] {
  if (analysis.batches.length === 0) return all;
  return analysis.batches.map(({ first, end, repeat }) => {
    const group = all.slice(first, end);
    const spans = analysis.statements.slice(first, end);
    const from = spans[0]!.start;
    const sql = text.slice(from, spans.at(-1)!.end);
    return {
      sql,
      offset: offset < 0 ? -1 : offset + from,
      readOnly: group.every((p) => p.readOnly),
      checks: group.flatMap((p) => p.checks),
      kind: groupKind(group),
      params: spans.flatMap((s) => s.parameters.map((p) => ({ ...p, start: p.start - from, end: p.end - from }))),
      depth: group[0]!.depth,
      file: group[0]!.file,
      repeat,
    };
  });
}

let nextResultId = 1;

/** A new result tab for `sql`, before it has a job. */
function newResult(sql: string, offset: number, status: ResultStatus, limit: number | null): Result {
  return { id: nextResultId++, sql, offset, jobId: null, columns: null, rows: 0, fetching: false, status, limit };
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
  connection: saved,
  active,
  ensureSession,
  currentSession,
  sideSession,
  catalog,
  onNeedCatalog,
  onChange,
  onError,
}: Props) {
  // A tab made read-only gets every check of a read-only connection.
  const connection = useMemo(() => (tab.readOnly ? { ...saved, read_only: true } : saved), [saved, tab.readOnly]);
  const editor = useRef<SqlEditorHandle>(null);
  const root = useRef<HTMLDivElement>(null);
  const [tx, setTx] = useState<TransactionState | null>(null);
  /** A run waiting for the answer to its checks. */
  const [guard, setGuard] = useState<{ request: GuardRequest; resolve: (run: boolean) => void } | null>(null);
  /** When the open transaction began, as far as the tab saw it. */
  const [txSince, setTxSince] = useState<number | null>(null);
  /** The statements run in the open transaction that change data or schema. */
  const [uncommitted, setUncommitted] = useState<string[]>([]);
  const [pendingOpen, setPendingOpen] = useState(false);
  /** What ran in the session, by transaction, for the transaction log. */
  const [txLog, setTxLog] = useState(emptyLog);
  const [logOpen, setLogOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const warnMinutes = useSetting(transactionWarning);
  const slowSeconds = useSetting(slowQuerySeconds);
  const warned = useRef(false);
  /** Count rows: the text shown, and the statement (`queryKey`) it counted. */
  const [count, setCount] = useState<{ key: string | null; text: string } | null>(null);
  const [exportOpen, setExportOpen] = useState(false);
  const [exporting, setExporting] = useState<ExportProgress | null>(null);
  const [results, setResults] = useState<Result[]>([]);
  const [activeResult, setActiveResult] = useState(0);
  const [running, setRunning] = useState(false);
  const shown = results[activeResult];
  /** The SQL field of the shown result; the grid shows its output in place of the result. */
  const resultSql = useResultSql({
    connection,
    results,
    shown,
    enabled: !isNoSql(connection.driver) && !tab.terminal,
    canLeave: () => discardEdits(),
  });
  /** The rows on screen: the shown result, or the output of its SQL field. */
  const view = resultSql.output ?? shown;
  // "Query with SQL" reads this tab's results with rows and which one is shown.
  const resultsNow = useRef({ results, shown, active, focusSql: resultSql.focus, pivot: resultSql.pivot, database: tab.database });
  resultsNow.current = { results, shown, active, focusSql: resultSql.focus, pivot: resultSql.pivot, database: tab.database };
  // An agent the user allows reads the editor and the grid as they are when it asks.
  const sharedNow = useRef<Omit<TabParts, "editor">>({ connection, title: null, result: view });
  sharedNow.current = { connection, title: tab.filePath ? basename(tab.filePath) : (tab.title ?? null), result: view };
  useEffect(
    () =>
      provideContext({
        tabKey: tab.key,
        snapshot: () => tabSnapshot({ ...sharedNow.current, editor: editor.current }),
      }),
    [],
  );
  useEffect(
    () =>
      provideResults({
        tabKey: tab.key,
        connection,
        shown: () => resultsNow.current.active,
        results: () => resultsWithRows(resultsNow.current.results),
        activeJob: () => resultsNow.current.shown?.jobId ?? null,
        querySql: () => resultsNow.current.focusSql(),
        pivot: () => resultsNow.current.pivot(),
        database: () => resultsNow.current.database ?? null,
      }),
    [],
  );
  /** Data viewer: the last statement found to be a read-only query the header sort can rewrite
   *  with certainty; asked once per statement, not at each keystroke. */
  const [rewritable, setRewritable] = useState<string | null>(null);
  useEffect(() => {
    if (!tab.table || shown === undefined) return;
    let live = true;
    const sql = shown.sql;
    api.sqlSortable(sql, connection.driver).then(
      (ok) => live && setRewritable(ok ? sql : null),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [tab.table, shown?.sql, connection.driver]);
  /** The shown rows come from such a query, still as it ran in the editor: the header sort and
   *  the quick filters can rewrite it. Typing in it turns them off until it runs again. */
  const sortable =
    shown?.status.kind === "rows" && rewritable === shown.sql && statementRange(shown, tab.sql) !== null;
  /** Data viewer: what the quick filters name for each column of a result (see
   *  `api.sqlFilterColumns`), asked once per result; `null` when it cannot be asked. */
  const [filters, setFilters] = useState<{ result: Result; columns: FilterTarget[] | null } | null>(null);
  useEffect(() => {
    if (!tab.table || !shown?.columns) return;
    let live = true;
    const result = shown;
    const done = (columns: FilterTarget[] | null) => live && setFilters({ result, columns });
    api.sqlFilterColumns(result.sql, connection.driver, shown.columns.map((c) => c.name)).then(done, () => done(null));
    return () => {
      live = false;
    };
  }, [tab.table, shown?.id, shown?.columns, connection.driver]);
  /** Those of the shown result; `undefined` until they are known. */
  const shownFilters =
    filters && filters.result.id === shown?.id && filters.result.columns === shown.columns ? filters.columns : undefined;
  /** The viewer's sort of the shown statement, for the header arrow and the next click. */
  const shownSort = tab.table && shown ? sortOf(tab.sorts, shown.sql) : null;
  /** The chart of the statement on screen, if one was made; it stays with the statement across
   *  runs. */
  const shownChart = view ? chartOf(tab.charts, view.sql) : null;
  const setChart = (patch: Partial<Omit<SavedChart, "query">>) => {
    if (view) onChange({ charts: withChart(tab.charts, view.sql, patch) });
  };
  // A count belongs to the statement it counted.
  const shownKey = shown ? queryKey(shown.sql) : null;
  const shownCount = count?.key === shownKey ? count.text : null;
  /** The run going on was started by auto-refresh. */
  const autoRun = useRef(false);
  /** The data viewer's own queries, to tell them from other queries typed in the tab. */
  const [viewerOwn] = useState(() =>
    viewerQueries(tab.viewerQuery, tab.table ? viewerSql(connection.driver, tab.table) : null),
  );
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
  // The editor's height, the same in every tab and kept for the next start.
  const [editorHeight, setEditorHeight] = useStoredSize("layout.editor_height", 200);
  /** The session's schema after the tab changed it (`SET search_path`); `null` for the
   *  catalog's. */
  const [schemaShown, setSchemaShown] = useState<string | null>(null);
  const runToken = useRef(0);
  const jobs = useRef<number[]>([]);
  const currentJob = useRef<number | null>(null);
  /** The gate of the statement `runOne` last ran, which Cancel and closing the tab end. */
  const statement = useRef<ReturnType<typeof runGate> | null>(null);
  /** Cancel was pressed during the run: no statement after the one it reached runs. */
  const cancelAsked = useRef(false);
  /** The data editor has changes that running again would lose. */
  const unsaved = useRef(false);
  /** Data viewer: the table's keys, loaded once and again after DDL. */
  const details = useRef<Promise<TableDetails> | null>(null);
  /** The table each query reads, for editing its results; cleared after DDL. */
  const sources = useRef(new Map<string, Promise<EditSource | null>>());
  /** What the runs ran, to run again unasked: only statements that passed their checks. */
  const memory = useRef(new RunMemory<Planned>());
  /** Parameter values answered in the last run, reused when it runs again. */
  const lastAnswers = useRef<Record<string, string>>({});
  /** The script running in the command-line client, which Cancel stops. */
  const cliRun = useRef<number | null>(null);
  /** SQL that undoes the last saved edits. */
  const [undoScript, setUndoScript] = useState<string | null>(null);
  /** Seconds between runs of the last statements again; `null` is off. */
  const [refreshEvery, setRefreshEvery] = useState<number | null>(null);
  /** "Compare with previous run": the result a run of the same statement replaces is kept. */
  const compare = useCompare(connection);

  const closeJobs = () => {
    for (const id of jobs.current) void api.closeQuery(id);
    jobs.current = [];
  };
  // Release results when the tab closes; a statement still waiting for the backend ends too,
  // and the run behind it stops there.
  useEffect(
    () => () => {
      runToken.current++;
      statement.current?.abandon(cancelled());
      closeJobs();
    },
    [],
  );

  // Without a password prompt the catalog can load right away; otherwise on first run.
  useEffect(() => {
    if (!asksPassword(connection)) onNeedCatalog(false);
  }, [connection.id]);
  // After `USE other` completion follows the session to its database.
  const shownDatabase = useRef(tab.database);
  useEffect(() => {
    if (shownDatabase.current === tab.database) return;
    shownDatabase.current = tab.database;
    onNeedCatalog(false);
  }, [tab.database]);

  useEffect(() => {
    if (!tab.autorun) {
      // A new editor takes the keys at once (Ctrl+T, New SQL script); a data viewer leaves
      // them where they were, e.g. in the navigator.
      if (active && !tab.table) editor.current?.focus();
      return;
    }
    onChange({ autorun: false });
    void run("statement");
  }, []);

  // A data viewer restored at start reads its table's rows again once shown, as it did when
  // opened: only while the editor holds one of the viewer's own queries, which only read.
  const restoreRows = useRef(!tab.autorun && !!tab.table && viewerOwn.has(tab.sql));
  useEffect(() => {
    if (!active || !restoreRows.current) return;
    restoreRows.current = false;
    void run("statement");
  }, [active]);

  // Changes to the results wait for one render every `RENDER_EVERY_MS`: a script of a thousand
  // statements would otherwise render every result tab for each event of each statement.
  const patches = useRef(new Map<number, ((r: Result) => Partial<Result>)[]>());
  /** The result tab to show once the changes are drawn; `null` leaves it. */
  const shownNext = useRef<number | null>(null);
  const flushTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const flush = () => {
    if (flushTimer.current !== null) clearTimeout(flushTimer.current);
    flushTimer.current = null;
    const pending = patches.current;
    if (pending.size > 0) {
      patches.current = new Map();
      setResults((all) =>
        all.map((r) => pending.get(r.id)?.reduce<Result>((acc, f) => ({ ...acc, ...f(acc) }), r) ?? r),
      );
    }
    const show = shownNext.current;
    shownNext.current = null;
    if (show !== null) setActiveResult(show);
  };
  const later = () => {
    flushTimer.current ??= setTimeout(flush, RENDER_EVERY_MS);
  };
  useEffect(() => () => {
    if (flushTimer.current !== null) clearTimeout(flushTimer.current);
  }, []);
  const update = (id: number, patch: Partial<Result> | ((r: Result) => Partial<Result>)) => {
    const list = patches.current.get(id) ?? [];
    list.push(typeof patch === "function" ? patch : () => patch);
    patches.current.set(id, list);
    later();
  };
  /** Shows the result tab at `index` with the next drawing of the changes. */
  const showResult = (index: number) => {
    shownNext.current = index;
    later();
  };

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
    if (mode === "script" || (from !== to && statements.some((s) => s.kind === "command"))) {
      return scriptOf(scope.text, analysis, all, scope.offset);
    }
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
   *  too (a fast statement finishes before `runQuery` returns the job), or when Cancel or
   *  closing the tab ends it before that; `seen` gets its job, columns and rows as they
   *  arrive. A statement with several results (a T-SQL batch, a MySQL CALL) gets a result
   *  tab for each after the first, placed by `onNext`; in a script, the run of the statement
   *  ends with its last result. */
  function runOne(
    sessionId: number,
    result: Result,
    token: number,
    history: boolean,
    seen?: Seen,
    onNext?: (child: Result, after: Result) => void,
  ): Promise<ResultStatus> {
    return new Promise((resolve) => {
      const gate = runGate(result.limit !== null, resolve);
      const { settle, jobKnown } = gate;
      statement.current = gate;
      // The result the events are about: the next one after a `next` event.
      let target = result;
      const onEvent = (event: QueryEvent) => {
        if (gate.abandoned()) return;
        if (token !== runToken.current) return settle({ kind: "skipped" });
        let status: ResultStatus | null = null;
        switch (event.type) {
          case "started":
            status = { kind: "rows", finished: false, complete: true, elapsedMs: event.elapsed_ms };
            if (seen) seen.columns = event.columns;
            update(target.id, { columns: event.columns, fetching: true, status });
            break;
          case "progress":
            if (seen) seen.rows = event.rows;
            update(target.id, { rows: event.rows, fetching: event.fetching });
            break;
          case "finished": {
            if (seen) seen.rows = event.rows;
            const rows: ResultStatus = {
              kind: "rows",
              finished: true,
              complete: event.complete,
              stopped: event.stopped,
              elapsedMs: event.elapsed_ms,
            };
            update(target.id, { rows: event.rows, fetching: false, status: rows });
            // Another result follows: the statement is not over.
            if (!event.more) status = rows;
            break;
          }
          case "affected":
            status = { kind: "affected", rows: event.rows, elapsedMs: event.elapsed_ms };
            update(target.id, { status });
            break;
          case "failed":
            status = { kind: "error", error: event.error };
            update(target.id, { status });
            break;
          case "messages":
            update(target.id, (r) => ({ messages: [...(r.messages ?? []), ...event.lines] }));
            break;
          case "next": {
            const child: Result = { ...newResult(result.sql, result.offset, { kind: "running" }, result.limit), jobId: event.job_id };
            jobs.current.push(event.job_id);
            currentJob.current = event.job_id;
            if (onNext) onNext(child, target);
            else setResults((all) => withAfter(all, target.id, [child]));
            if (seen) Object.assign(seen, { jobId: event.job_id, columns: null, rows: 0, resultId: child.id });
            target = child;
            break;
          }
        }
        if (status) settle(status);
      };
      api.runQuery(sessionId, result.sql, result.limit, onEvent, history).then(
        (jobId) => {
          // The tab closed before the backend answered: the job it started has no owner, and
          // its statement may be running.
          if (gate.abandoned()) {
            void api
              .cancelQuery(jobId)
              .catch(() => undefined)
              .then(() => api.closeQuery(jobId));
            return;
          }
          jobs.current.push(jobId);
          // A later result's job may be known already.
          if (target === result) currentJob.current = jobId;
          if (seen && seen.jobId === null) seen.jobId = jobId;
          update(result.id, { jobId });
          jobKnown();
          // Cancel came while the backend was starting the job (e.g. reconnecting): it stops the
          // statement now, unless that has ended already.
          if (gate.cancelDue()) void api.cancelQuery(jobId).catch((e: unknown) => onError(toCommandError(e).message));
        },
        (e: unknown) => {
          if (gate.abandoned()) return;
          const status: ResultStatus = { kind: "error", error: toCommandError(e) };
          update(result.id, { status });
          jobKnown();
          settle(status);
        },
      );
    });
  }

  /** Marks where `planned` failed. The server counts the characters of what it received,
   *  `sent`: `planned` with the parameter `values` filled in, where JavaScript counts UTF-16
   *  units. */
  function showError(planned: Planned, status: ResultStatus, sent = planned.sql, values: Record<string, string> = {}) {
    if (status.kind !== "error" || status.error.kind === "cancelled" || planned.offset < 0) return;
    const text = editor.current!.getText();
    const position = status.error.position;
    const inSent = position ? utf16Offset(sent, position - 1) : null;
    const inPlanned =
      inSent === null || sent === planned.sql ? inSent : originalOffset(planned.sql, planned.params, values, connection.driver, inSent);
    const from = planned.offset + (inPlanned ?? 0);
    const word = /^[\w$"]+/.exec(text.slice(from));
    editor.current!.setError({
      from,
      to: inPlanned !== null ? from + (word?.[0].length ?? 1) : planned.offset + planned.sql.length,
      message: status.error.message,
    });
  }

  /** After `USE other` (MySQL, SQL Server), `use other` (MongoDB) or a change of PostgreSQL's
   *  `search_path`, the tab shows, completes and reopens where the session now is. */
  async function followContext(sessionId: number, ran: Planned[]) {
    let database: string | null = null;
    let schema = false;
    for (const p of ran) {
      database = usedDatabase(p.sql, connection.driver) ?? database;
      schema ||= setsSchema(p.sql);
    }
    if (database !== null && database !== (tab.database ?? null)) {
      onChange({ database });
      setSchemaShown(null);
    }
    if (schema) {
      const schemas = await api.listSchemas(sessionId).catch(() => null);
      setSchemaShown(schemas?.find((s) => s.current)?.name ?? null);
    }
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

  /** `p`'s SQL with its parameters filled in, outside a run (Explain, Count rows): the tab's
   *  variables and the last answers, asking for the rest; null when the user cancels. */
  async function filledIn(p: Planned): Promise<string | null> {
    if (!p.params.length) return p.sql;
    const values = await fillParams(p, { ...(tab.variables ?? {}), ...lastAnswers.current }, new Set());
    return values && substitute(p.sql, p.params, values, connection.driver);
  }

  /** Exports the rows of `sql` to `path` instead of showing them (`@export`). */
  function exportTo(sessionId: number, sql: string, target: ExportTarget): Promise<ResultStatus> {
    const { path, format, delimiter, compression } = target;
    return new Promise((resolve) => {
      // SQL statements insert into a table named after the file.
      const table = path.split(/[\\/]/).pop()!.replace(/(\.\w+)?(\.(gz|zst))?$/i, "");
      const options: ExportOptions = { format, delimiter, header: true, table, driver: connection.driver, compression };
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

  /** Runs what `mode` selects in the editor, or `again` (statements that ran before). With
   *  `replace`, the result of that id gives way to the new one and the others stay.
   *  An `auto` refresh stays out of the query history, keeps the result tab shown and the
   *  editor as it is, and stops refreshing when a statement fails. */
  async function run(mode: RunMode, force = false, again?: Planned[], auto = false, replace?: number) {
    const history = !auto;
    if (running && !force) return;
    if (!(await discardEdits())) return;
    const append = mode === "append";
    let planned: Planned[];
    try {
      planned = again ?? (await plan(append ? "statement" : mode));
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
    // Statements that only read go again without asking; anything else is asked again.
    if (!(again && rerunsUnasked(again)) && !(await confirmed(planned))) return;
    const sessionId = await ensureSession();
    if (sessionId === null) return;
    if (!again) setUndoScript(null);
    const replaced = replace === undefined ? undefined : results.find((r) => r.id === replace);
    // Remembered only now that it passed its checks: a declined run never runs again unasked.
    if (replaced) memory.current.forget(replaced.id);
    else memory.current.started(planned, !append);
    onNeedCatalog(false);
    if (!append) await compare.keep(sessionId, planned, replaced ? [replaced] : results);

    const token = ++runToken.current;
    if (replaced) {
      if (replaced.jobId !== null) {
        void api.closeQuery(replaced.jobId);
        jobs.current = jobs.current.filter((j) => j !== replaced.jobId);
      }
    } else if (!append) {
      closeJobs();
    }
    const script = mode === "script" && planned.length > 1;
    const limit = tab.terminal ? TERMINAL_ROWS : script ? getSetting(scriptRowLimit) : null;
    const resultOf = (p: Planned, first: boolean) =>
      newResult(p.sql, p.offset, { kind: first ? "running" : "pending" }, limit);
    // `queue` grows when an `@include` adds a file's statements after itself.
    const queue = [...planned];
    const own = queue.map((p, i) => resultOf(p, i === 0));
    const base = append ? results.length : replaced ? Math.max(0, results.indexOf(replaced)) : 0;
    // A copy: React runs the update later, when `own` may have grown with the results an
    // `@include` or a further result set adds by updates of their own.
    const initial = [...own];
    setResults((all) => {
      if (append) return [...all, ...initial];
      if (!replaced) return initial;
      const at = all.findIndex((r) => r.id === replaced.id);
      return at < 0 ? [...all, ...initial] : [...all.slice(0, at), ...initial, ...all.slice(at + 1)];
    });
    if (!auto) setActiveResult(base);
    autoRun.current = auto;
    cancelAsked.current = false;
    setRunning(true);
    editor.current!.setError(null);

    // A run again after saving edited rows reuses the values answered last time.
    if (!again) lastAnswers.current = {};
    const variables = { ...(tab.variables ?? {}), ...lastAnswers.current };
    const defined = new Set<string>(again ? Object.keys(variables) : []);
    const keepVariables = () =>
      onChange({ variables: Object.fromEntries(Object.entries(variables).filter(([k]) => !k.startsWith("?"))) });
    // Set by `@export`, taken by the next statement.
    const pending: { export: ExportTarget | null } = { export: null };

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
          const path = resolvePath(c.path, p.file ?? tab.filePath);
          if (!path) return { kind: "error", error: clientError(t("Save the script first, or give @export an absolute path.")) };
          pending.export = { path, format: c.format, delimiter: c.delimiter, compression: c.compression };
          return { kind: "message", summary: t("Next rows to {path}", { path: c.path }), text: t("The next statement's rows go to {path}.", { path }) };
        }
        case "error":
          return { kind: "error", error: clientError(c.message) };
        case "include": {
          // A relative path in an included file is next to that file, as psql's \ir reads it.
          const path = resolvePath(c.path, p.file ?? tab.filePath);
          if (!path) return { kind: "error", error: clientError(t("Save the script first, or give @include an absolute path.")) };
          if (p.depth >= INCLUDE_DEPTH) {
            return { kind: "error", error: clientError(t("@include nests more than {n} files.", { n: INCLUDE_DEPTH })) };
          }
          try {
            const text = await api.readTextFile(path);
            const analysis = await api.analyzeSql(text, connection.driver, 0);
            const added = scriptOf(text, analysis, plannedOf(text, analysis.statements, -1, p.depth + 1, path), -1);
            // The file's statements get the checks the editor's had before the run.
            if (connection.read_only && added.some((a) => !a.readOnly)) {
              return { kind: "error", error: clientError(t("{name} is read-only and {path} may modify data.", { name: connection.name, path })) };
            }
            if (!(await confirmed(added))) return cancelled();
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
    let failed: number | null = null;
    let lastRows: number | null = null;
    /** Results of a statement after its first (a T-SQL batch, a MySQL CALL): placed after
     *  it in `own`, with their statement repeated in `queue`, and not run again. */
    const extra = new Set<number>();
    const onNext = (child: Result, after: Result) => {
      const at = own.findIndex((r) => r.id === after.id);
      own.splice(at + 1, 0, child);
      queue.splice(at + 1, 0, queue[at]!);
      extra.add(child.id);
      setResults((all) => withAfter(all, after.id, [child]));
    };
    let flashed = 0;
    for (let i = 0; i < queue.length; i++) {
      const result = own[i]!;
      const p = queue[i]!;
      if (token !== runToken.current) return;
      if (extra.has(result.id)) continue;
      update(result.id, { status: { kind: "running" } });
      if (!auto) {
        showResult(base + i);
        // A long script would spend its time moving the highlight.
        if (p.offset >= 0 && performance.now() - flashed > 150) {
          flashed = performance.now();
          editor.current!.flash(p.offset, p.offset + p.sql.length);
        }
      }
      const seen: Seen = { jobId: null, columns: null, rows: 0 };
      let status: ResultStatus;
      let sql = p.sql;
      let values: Record<string, string> = variables;
      /** The snapshot taken of the rows the statement changes. */
      let snapshot: string | null = null;
      if (p.kind === "command") {
        status = await command(p, i);
      } else {
        const filled = p.params.length ? await fillParams(p, variables, defined) : variables;
        if (!filled) {
          status = cancelled();
        } else {
          values = filled;
          if (p.params.length) {
            sql = substitute(p.sql, p.params, filled, connection.driver);
            Object.assign(variables, filled);
            keepVariables();
          }
          memory.current.ran(result.id, p);
          const autoLimit = !tab.table && p.kind === "query" ? getSetting(autoLimitSetting) : 0;
          const capped = autoLimit > 0 ? await api.sqlAddLimit(sql, connection.driver, autoLimit).catch(() => null) : null;
          // The result keeps the statement as written: the header sort and the editor find it.
          const ran = { ...result, sql: capped ?? sql };
          update(result.id, { sql, autoLimit: capped ? autoLimit : null });
          const target = pending.export;
          pending.export = null;
          if (target) {
            status = await exportTo(sessionId, sql, target);
          } else {
            // The rows an UPDATE or DELETE changes, kept before it runs so that they can be put back.
            if (p.kind === "dml" && !isNoSql(connection.driver)) snapshot = await snapshotBefore(sessionId, tab.database ?? null, sql);
            // `GO 3` runs the batch three times, until one fails, each to its end.
            const repeat = p.repeat ?? 1;
            const each = repeat > 1 && ran.limit === null ? { ...ran, limit: getSetting(scriptRowLimit) } : ran;
            status = { kind: "skipped" };
            for (let n = 0; n < repeat; n++) {
              status = await runOne(sessionId, each, token, history, seen, onNext);
              // A dropped connection fails before the statement reaches the server; the
              // second attempt runs on a reopened connection, which says it was reopened.
              if (status.kind === "error" && status.error.kind === "closed" && token === runToken.current && !cancelAsked.current) {
                update(result.id, { status: { kind: "running" } });
                status = await runOne(sessionId, each, token, history, seen, onNext);
              }
              if (status.kind === "error" || cancelAsked.current || token !== runToken.current) break;
            }
          }
        }
      }
      if (token !== runToken.current) return;
      // The result the statement's last status belongs to: its last one when it had several.
      const last = seen.resultId ?? result.id;
      // runOne reports the statuses of a job; commands, exports and cancelled ones are set here.
      if (seen.jobId === null) update(result.id, { status });
      // Whether a transaction is open after it, for the log; a query changes nothing.
      let open: boolean | null = null;
      if (p.kind !== "query" && p.kind !== "command" && status.kind !== "error" && status.kind !== "skipped") {
        open = (await notePending(sessionId, p.kind, sql))?.open ?? null;
      } else if (status.kind === "error" && p.kind !== "command") {
        open = (await api.transactionState(sessionId).catch(() => null))?.open ?? null;
      }
      if (p.kind !== "command" && status.kind !== "skipped") {
        const failed = status.kind === "error";
        // A statement that failed changed nothing: its snapshot would restore rows twice.
        if (failed && snapshot) void api.deleteSnapshot(snapshot).catch(() => {});
        const entry = { at: Date.now(), sql, kind: p.kind, failed, snapshot: failed ? null : snapshot };
        setTxLog((log) => noteStatement(log, entry, open));
      }
      if (tab.terminal) await logTerminal(last, sql, status, seen);
      if (status.kind === "rows") lastRows = last;
      if (status.kind === "error") {
        failed = last;
        // The editor may have changed since the statements were planned: no marker there.
        if (!auto) showError(p, status, sql, values);
        else onError(t("Auto-refresh stopped: {message}", { message: status.error.message }));
      }
      // After an error, or a Cancel that came too late to stop this statement, nothing else runs.
      if (status.kind === "error" || cancelAsked.current) {
        const skipped = new Set(own.slice(i + 1).filter((r) => !extra.has(r.id)).map((r) => r.id));
        setResults((all) => all.map((r) => (skipped.has(r.id) ? { ...r, status: { kind: "skipped" } } : r)));
        break;
      }
    }
    if (auto && failed !== null) setRefreshEvery(null);
    shownNext.current = null;
    flush();
    const finalId = failed ?? lastRows ?? own.at(-1)?.id;
    if (!auto || failed !== null) setActiveResult(base + Math.max(0, own.findIndex((r) => r.id === finalId)));
    setRunning(false);
    currentJob.current = null;
    if (queue.some((p) => DDL.test(p.sql))) {
      // The navigator, open object tabs and autocompletion (App) reload what changed.
      requestNavigatorRefresh({ connectionId: connection.id, database: tab.database ?? null });
      details.current = null;
      sources.current.clear();
    }
    setHistoryVersion((v) => v + 1);
    await refreshTx(sessionId);
    if (!auto && failed === null) await followContext(sessionId, queue);
  }
  /** Runs the selection, or the whole editor, with the database's command-line client. */
  async function runCli() {
    if (running) return;
    const ed = editor.current!;
    const { from, to } = ed.getSelection();
    const script = from !== to ? ed.getText().slice(from, to) : ed.getText();
    if (!script.trim()) return;
    let planned: Planned[];
    try {
      planned = plannedOf(script, (await api.analyzeSql(script, connection.driver, 0)).statements, 0);
    } catch (e) {
      onError(toCommandError(e).message);
      return;
    }
    if (saved.read_only && planned.some((p) => !p.readOnly)) {
      onError(t("{name} is read-only and this may modify data.", { name: connection.name }));
      return;
    }
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
        // A routine is listed with its arguments; a table's name may hold a `(` too.
        const base = (o: ObjectMeta) =>
          o.kind === "function" || o.kind === "procedure" ? o.name.replace(/\(.*$/, "") : o.name;
        const object = objects.find((o) => base(o) === name) ?? objects.find((o) => same(base(o), name));
        if (object) {
          openObjectTab(connection.id, {
            database: tab.database,
            schema: s.name,
            currentSchema: s.current,
            name: object.name,
            kind: object.kind,
            parent: object.parent,
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
    const sql = await filledIn(p);
    if (sql === null) return;
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
    // Not on a temporary connection ("Query with SQL"), nor MongoDB's and Redis's scripts.
    runOnConnections:
      tab.resultSource || tab.across || isNoSql(connection.driver) || !workbench().connections().some((c) => c.id === connection.id)
        ? undefined
        : () => {
            const { from, to } = editor.current!.getSelection();
            const text = editor.current!.getText();
            const script = from === to ? text : text.slice(from, to);
            workbench().openDialog((close) => <MultiRunDialog script={script} origin={connection} onClose={close} />);
          },
    explain: isNoSql(connection.driver) ? undefined : (analyze) => void explain(analyze),
  };

  /** Closes a result tab and frees its rows; the shown one asks first if it has edits. */
  function closeResult(id: number) {
    const i = results.findIndex((r) => r.id === id);
    const r = results[i];
    // The run in progress places its results by position.
    if (!r || running) return;
    if (i === activeResult && unsaved.current) return leavingEdits(() => dropResult(r, i));
    dropResult(r, i);
  }

  function dropResult(r: Result, i: number) {
    if (r.jobId !== null) {
      void api.closeQuery(r.jobId);
      jobs.current = jobs.current.filter((j) => j !== r.jobId);
    }
    memory.current.forget(r.id);
    setResults((all) => all.filter((x) => x.id !== r.id));
    setActiveResult((a) => Math.max(0, a > i || a === results.length - 1 ? a - 1 : a));
  }

  /** After a statement that is not a query: a change made while a transaction stays open
   *  waits for Commit and is listed; one that leaves none open (a COMMIT) empties the list. */
  async function notePending(sessionId: number, kind: StatementSpan["kind"], sql: string) {
    const state = await api.transactionState(sessionId).catch(() => null);
    if (!state) return null;
    // Unchanged states keep their objects: a script of many statements renders less.
    setTx((now) => (now && now.open === state.open && now.autocommit === state.autocommit ? now : state));
    const change = kind === "dml" || kind === "ddl";
    setUncommitted((list) => (!state.open ? (list.length ? [] : list) : change ? [...list, sql] : list));
    return state;
  }

  /** Opens the differences between the shown result and the previous run of its statement,
   *  rows matched by the table's key when the grid could edit them (the same rule). */
  async function compareShown() {
    const result = shown;
    if (!result?.columns || result.jobId === null) return;
    const sessionId = await ensureSession();
    if (sessionId === null) return;
    const source = await (editTarget(result)?.load() ?? Promise.resolve(null)).catch(() => null);
    const meta = source ? editMeta(source, result.columns, connection.virtual_keys ?? []) : null;
    const key = meta && "key" in meta ? meta.key : null;
    try {
      await compare.open(sessionId, result.jobId, key, tab.database ?? null);
    } catch (e) {
      onError(toCommandError(e).message);
    }
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
      setTxLog((log) => noteEnd(log, commit, Date.now()));
      onChange({ txOpen: state.open });
    } catch (e) {
      onError(toCommandError(e).message);
      await refreshTx(sessionId);
    }
  }

  /**
   * Data viewer: rewrites the statement that produced `result` in the editor (only that
   * statement, the rest of the text stays) and runs it. Returns the new statement; null when
   * the editor no longer holds the statement or the rewrite cannot place the change with
   * certainty.
   */
  async function rewriteStatement(result: Result, rewrite: (sql: string) => Promise<string | null>): Promise<string | null> {
    const ed = editor.current!;
    let next: string | null = null;
    try {
      next = statementRange(result, ed.getText()) ? await rewrite(result.sql) : null;
    } catch (e) {
      onError(toCommandError(e).message);
      return null;
    }
    // Looked up again: the user may have typed while the rewrite was on its way.
    const range = statementRange(result, ed.getText());
    if (!range || next === null) {
      onError(t("This query cannot be changed from the grid: edit it in the editor."));
      return null;
    }
    if (viewerOwn.rewritten(result.sql, next)) onChange({ viewerQuery: next });
    ed.replace(range.from, range.to, next);
    // The cursor inside the statement, so that it is the one that runs.
    const inside = range.from + Math.floor(next.length / 2);
    ed.select(inside, inside);
    void run("statement");
    return next;
  }

  /** Data viewer: once edited rows are saved or discarded, rewrites the shown result's statement
   *  and runs it. `sort` is the viewer's sort of the new statement, `null` for none. */
  function rewriteShown(rewrite: (sql: string) => Promise<string | null>, sort: ShownSort | null) {
    const result = shown;
    if (!tab.table || !result) return;
    const go = () =>
      void rewriteStatement(result, rewrite).then((next) => {
        if (next !== null) onChange({ sorts: withSort(tab.sorts, next, sort) });
      });
    leavingEdits(() => {
      // Asked already: the run that follows does not ask again.
      unsaved.current = false;
      go();
    });
  }

  /** Whether the edits of the shown result may go, asking first when there are some: running,
   *  showing another result, the chart or the terminal drops them, as closing the tab does. */
  async function discardEdits(): Promise<boolean> {
    return !unsaved.current || ask(t(DISCARD_EDITS), { confirm: t("Discard"), danger: true });
  }

  /** Does `then` once the edits of the shown result may go. */
  function leavingEdits(then: () => void) {
    void discardEdits().then((yes) => yes && then());
  }

  /** Header clicks cycle a column of the shown statement through ascending, descending and
   *  back to the ORDER BY the statement had before the first click, or unsorted. */
  async function sortBy(column: string) {
    if (!shown) return;
    const statement = shown.sql;
    const { sort, original } = await clickSort(shownSort, column, () =>
      api.sqlOrderClause(statement, connection.driver).catch(() => null),
    );
    rewriteShown((sql) => api.sqlOrderBy(sql, connection.driver, sort?.column ?? null, sort?.desc ?? false, original), sort);
  }

  /** Quick filter of the grid: adds `condition` to the WHERE of the shown result's statement,
   *  which keeps its ORDER BY, and so its sort. */
  function addCondition(condition: string) {
    rewriteShown((sql) => api.sqlAddCondition(sql, connection.driver, condition), shownSort);
  }

  /** What Count rows counts: the query of the shown rows as it ran, its variables filled in;
   *  else the statement under the cursor, filled in as a run would. Null when there is none or
   *  the user cancels. */
  async function countedSql(): Promise<string | null> {
    if (shown?.status.kind === "rows") return shown.sql;
    try {
      const [p] = await plan("statement");
      return p ? await filledIn(p) : null;
    } catch (e) {
      onError(toCommandError(e).message);
      return null;
    }
  }

  async function countRows() {
    if (!tab.table) return;
    // The count shows next to the statement it counted only: another one shown meanwhile, or
    // a later count, hides it.
    const key = shownKey;
    const sql = await countedSql();
    if (sql === null) return;
    const mine = { key, text: t("counting…") };
    setCount(mine);
    const show = (text: string | null) => setCount((c) => (c !== mine ? c : text === null ? null : { key, text }));
    const sessionId = await sideSession();
    if (sessionId === null) return show(null);
    try {
      const query = await api.sqlCountQuery(sql, connection.driver);
      if (query === null) {
        show(null);
        return onError(t("This query cannot be counted."));
      }
      const value = await api.queryValue(sessionId, query);
      show(value === null ? null : plural(Number(value), "{n} row", "{n} rows"));
    } catch (e) {
      show(null);
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
    // The output of the SQL field runs again on the copy it was read from, a session that runs
    // one statement at a time: the output keeps the rows it has read.
    const output = result === resultSql.output;
    if (output && result.jobId !== null) await api.stopFetch(result.jobId);
    const sessionId = output ? resultSql.sessionId : await ensureSession();
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
    // The run is over, but the shown result is still reading rows from a slow server.
    if (!running) {
      const job = shown?.jobId;
      if (job != null) await api.cancelQuery(job).catch((e: unknown) => onError(toCommandError(e).message));
      return;
    }
    const job = currentJob.current;
    const cli = cliRun.current;
    cancelAsked.current = true;
    // `run_query` has not answered (it may be reconnecting): there is no job to cancel yet, and
    // the statement may still start. Its job is cancelled once known (see `runOne`).
    const waiting = statement.current;
    if (cli === null && waiting?.awaitsJob()) {
      waiting.requestCancel();
      return;
    }
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
    // The data editor writes SQL, or Redis commands: MongoDB results stay read-only.
    if (isMongo(connection.driver)) return null;
    const ref = tab.tableRef;
    const viewer = !!tab.table && !!ref && viewerOwn.has(result.sql);
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
      // The saved connection: a tab made read-only says so instead of blaming the connection.
      connection: saved,
      database: tab.database,
      key: viewer ? `table:${tab.table}` : `sql:${result.sql}`,
      quiet: !viewer,
      readOnly: tab.readOnly ? msg("the tab is read-only") : null,
      load,
      session: sideSession,
      onSaved: (undo) => {
        setUnsaved(false);
        setUndoScript(undo);
        // The statement of the edited result runs again in its place, the other results stay;
        // only a statement that reads runs unasked. The grid that had the focus (Ctrl+S) gets
        // it back once its rows are drawn again.
        const focus = document.activeElement;
        const inGrid = !focus || focus === document.body || !!focus.closest(".grid-scroll");
        const again = memory.current.readOf(result.id);
        if (!again) return;
        void run("statement", false, [again], false, result.id).then(() => {
          if (!inGrid || (document.activeElement && document.activeElement !== document.body)) return;
          root.current?.querySelector<HTMLElement>(".grid-scroll")?.focus();
        });
      },
      onDirtyChange: setUnsaved,
    };
  }

  /** The viewer's table, for the notes in the grid header; none for other results. */
  function notesScope(result: Result): NotesScope | null {
    const ref = tab.tableRef;
    if (!(tab.table && ref) || isMongo(connection.driver)) return null;
    if (!viewerOwn.has(result.sql)) return null;
    return { connectionId: connection.id, database: tab.database, schema: ref.schema, table: ref.name };
  }

  // The open transaction's clock, and one warning when it has been open too long.
  useEffect(() => {
    if (!tx?.open) {
      setTxSince(null);
      setUncommitted([]);
      setPendingOpen(false);
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
    const last = memory.current.last();
    if (running || guard || unsaved.current || last.length === 0) return;
    const again = memory.current.lastReads();
    if (!again) {
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
  // ClickHouse, MongoDB and Redis have no transactions (`capabilities.transactions` is false):
  // no commit mode.
  // The data viewer's bar, with its row count: not on Redis, whose rows are read as shown.
  const viewerBar = !!tab.table && !isRedis(connection.driver);
  /** The table the data viewer browses, for its structure and its editor. */
  const viewed = tab.table && tab.tableRef ? { ...tab.tableRef, database: tab.database } : null;
  const showTx = toolbar.visible.transactions && connection.driver !== "clickhouse" && !isNoSql(connection.driver);
  const dirty = tab.filePath !== null && tab.sql !== tab.savedSql;
  const rowsResult = view?.status.kind === "rows" && view.columns ? view : null;
  const manual = tx !== null && !tx.autocommit;
  /** The shown result's rows still come from the server, slowly: Cancel stops them. */
  const readingRows = shown?.status.kind === "rows" && !shown.status.finished && shown.fetching && shown.jobId !== null;
  const exportBusy = exporting !== null && exporting.done === null;

  return (
    <div className="query-tab" ref={root} style={{ display: active ? "flex" : "none" }}>
      <div
        className="query-main"
        // The editor gives up height before the toolbar does in a small window.
        style={{ gridTemplateRows: `auto ${viewerBar ? "auto " : ""}minmax(60px, ${editorHeight}px) 6px 1fr auto` }}
      >
        <div className="toolbar">
          <button
            className="primary"
            onClick={() => void run("statement")}
            title={titleWithKeys(t("Run statement"), "editor.run_statement")}
            aria-label={t("Run")}
          >
            <Icon name="run" />
            {t("Run")}
          </button>
          {toolbar.visible.script && (
            <button onClick={() => void run("script")} title={titleWithKeys(t("Run script"), "editor.run_script")} aria-label={t("Script")}>
              {toolbar.label("script", t("Script"))}
            </button>
          )}
          {toolbar.visible.cancel && (
            <button
              onClick={() => void cancel()}
              disabled={!running && !readingRows}
              title={t("Cancel the running statement")}
              aria-label={t("Cancel")}
            >
              {toolbar.label("cancel", t("Cancel"))}
            </button>
          )}
          <ConnectionChip connection={connection} database={tab.database ?? null} schema={schemaShown ?? catalog?.default_schema ?? null} />
          <button
            className={connection.read_only ? "toggled" : ""}
            onClick={() => onChange({ readOnly: !tab.readOnly })}
            disabled={saved.read_only}
            aria-label={t("Read-only")}
            aria-pressed={connection.read_only}
            title={t(
              saved.read_only
                ? "The connection is read-only"
                : tab.readOnly
                  ? "Read-only in this tab: click to allow changes again"
                  : "Make this tab read-only, without changing the connection",
            )}
          >
            {toolbar.label("lock", t("Read-only"))}
          </button>
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
          {showTx && (
            <button onClick={() => setLogOpen(true)} title={t("Transaction log")} aria-label={t("Transaction log")}>
              {toolbar.label("tx-log", t("Log"))}
            </button>
          )}
          {tx?.open && (
            <button
              type="button"
              className={`tx-open${txLong ? " long" : ""}`}
              onClick={() => setPendingOpen(true)}
              title={t("A transaction is open: changes are not visible to others yet, and locks are held")}
            >
              ● {t("Transaction open {time}", { time: elapsed(txAge) })}
              {uncommitted.length > 0 && ` · ${plural(uncommitted.length, "{n} change", "{n} changes")}`}
            </button>
          )}
          {toolbar.visible.files && (
            <>
              <span className="divider" />
              <button onClick={() => void openFile()} title={titleWithKeys(t("Open script"), "editor.open")} aria-label={t("Open")}>
                {toolbar.label("open", t("Open"))}
              </button>
              <button onClick={() => void saveFile()} title={titleWithKeys(t("Save script"), "editor.save")} aria-label={t("Save")}>
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
                onClick={() => (tab.terminal ? onChange({ terminal: false }) : leavingEdits(() => onChange({ terminal: true })))}
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
        {viewerBar && (
          <div className="viewer-bar">
            {viewed && (
              <button
                type="button"
                onClick={() =>
                  openObjectTab(connection.id, {
                    ...viewed,
                    // Named as the viewer was opened: bare in the session's current schema.
                    currentSchema: tab.table === qualifiedName(connection.driver, { name: viewed.schema, current: true }, viewed.name),
                  })
                }
                title={t("Open the details: properties, columns, keys and DDL")}
              >
                {t("Structure")}
              </button>
            )}
            {viewed?.kind === "table" && editsTables(connection.driver) && (
              <button
                type="button"
                onClick={() => openTableEditor(connection.id, viewed)}
                disabled={connection.read_only}
                title={t(connection.read_only ? "Read-only: the table cannot be changed from here" : "Edit the columns, keys, indexes and constraints of the table")}
              >
                {t("Edit table…")}
              </button>
            )}
            <span className="spacer" />
            {shownCount && <span className="muted small">{shownCount}</span>}
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
        <PanelResizer
          storeKey="layout.editor_height"
          size={editorHeight}
          fallback={200}
          min={60}
          max={() => window.innerHeight - 200}
          direction="row"
          onResize={setEditorHeight}
          label={t("Resize the editor")}
          className="splitter"
        />
        <div className="results">
          {tab.across && (
            <AcrossBar
              origin={tab.across}
              connection={connection}
              onReloaded={(across) => {
                onChange({ across });
                onNeedCatalog(true);
                // The last statements run again on the new rows, if they only read.
                const again = memory.current.lastReads();
                if (again) void run("statement", false, again);
              }}
            />
          )}
          {tab.savedResult && <SavedResultBar saved={tab.savedResult} />}
          {tab.resultSource && (
            <ResultSourceBar
              source={tab.resultSource}
              connection={connection}
              onReloaded={(source) => {
                onChange({ resultSource: source });
                onNeedCatalog(true);
                // The last statements run again on the new rows, if they only read.
                const again = memory.current.lastReads();
                if (again) void run("statement", false, again);
              }}
            />
          )}
          {blocked !== null && (
            <div className="notice warn">
              {t("{name} is read-only and this may modify data.", { name: connection.name })}
              {/* Only this tab is read-only: the connection, like every other entry point, refuses. */}
              {!saved.read_only && <button onClick={() => void run(blocked, true)}>{t("Run anyway")}</button>}
            </div>
          )}
          {tab.terminal && <TerminalView blocks={terminalLog} onClear={() => setTerminalLog([])} />}
          {!tab.terminal && results.length > 1 && (
            <div className="result-tabs" onWheel={scrollSideways}>
              {results.map((r, i) => (
                <button
                  key={r.id}
                  className={`result-tab ${r.status.kind}${i === activeResult ? " active" : ""}`}
                  onClick={() => i !== activeResult && leavingEdits(() => setActiveResult(i))}
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
          {!tab.terminal && view && resultSql.output && (
            <ResultPanel
              key={view.id}
              result={view}
              source={{ connection: resultSql.connection ?? connection, sql: view.sql, jobId: view.jobId, table: null }}
              chart={shownChart?.shown ? { config: shownChart.config, onConfig: (config) => setChart({ config }) } : null}
              toolbar={resultSql.bar}
            />
          )}
          {!tab.terminal && shown && !resultSql.output && (
            <ResultPanel
              key={shown.id}
              result={shown}
              sort={shownSort}
              onHeaderClick={tab.table && sortable && !running ? sortBy : undefined}
              headerHint={
                tab.table && !sortable && shown.status.kind === "rows"
                  ? rewritable === shown.sql
                    ? t("This query cannot be sorted from the header: the editor does not hold it as it ran (it was edited since, its variables were filled in, or it comes from @include).")
                    : t("This query cannot be sorted from the header: it is not one read-only query that an ORDER BY can be added to.")
                  : undefined
              }
              source={{
                connection,
                sql: shown.sql,
                jobId: shown.jobId,
                table: tab.table ?? null,
                viewer: tab.table && sortable ? { addCondition, columns: shownFilters } : undefined,
              }}
              edit={editTarget(shown)}
              notes={notesScope(shown)}
              chart={shownChart?.shown ? { config: shownChart.config, onConfig: (config) => setChart({ config }) } : null}
              toolbar={resultSql.bar}
            />
          )}
        </div>
        <div className="statusbar" role="status">
          {view && (
            <>
              <span>{describe(view)}</span>
              {rowsResult && (
                <ChartToggle
                  chart={!!shownChart?.shown}
                  onChange={(on) => (on ? leavingEdits(() => setChart({ shown: true })) : setChart({ shown: false }))}
                />
              )}
              {(view.status.kind === "rows" || view.status.kind === "affected") && (
                <span className="muted">
                  {t(view.status.kind === "rows" && !view.status.finished ? "first rows in {time}" : "took {time}", {
                    time: formatMs(view.status.elapsedMs),
                  })}
                </span>
              )}
              {(view.status.kind === "rows" || view.status.kind === "affected") &&
                isSlow(view.status.elapsedMs, slowSeconds) && (
                  <span
                    className="slow-query"
                    title={t("Slower than {n} s (Settings). Explain (Ctrl+Shift+E) shows where the time goes.", { n: slowSeconds })}
                  >
                    {t("slow")}
                  </span>
                )}
              {view.status.kind === "rows" && !view.status.finished && (
                <button className="link" onClick={() => fetchAll(view)}>
                  {t("Fetch all")}
                </button>
              )}
              {rowsResult && !exportBusy && !keepsDataIn(saved) && (
                <button className="link" onClick={() => setExportOpen(true)}>
                  {t("Export…")}
                </button>
              )}
              {compare.kept(view) && (
                <button
                  className="link"
                  disabled={!compare.comparable(view)}
                  title={compare.comparable(view) ? undefined : t("The columns changed since the previous run.")}
                  onClick={() => void compareShown()}
                >
                  {t("Compare with previous run")}
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
                onClick={() => workbench().openTab("query", connection.id, { sql: undoScript, database: tab.database })}
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
      {logOpen && (
        <TransactionLogDialog
          log={txLog}
          onRestore={(id) => {
            setLogOpen(false);
            void openRestore(connection.id, tab.database ?? null, id);
          }}
          onClear={() => setTxLog(emptyLog())}
          onClose={() => setLogOpen(false)}
        />
      )}
      {pendingOpen && (
        <PendingDialog
          statements={uncommitted}
          running={running}
          onEnd={(commit) => {
            setPendingOpen(false);
            if (commit !== null) void endTransaction(commit);
          }}
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
          // The SQL field's output runs on a copy of its own: it cannot be kept as a profile.
          query={rowsResult === resultSql.output ? undefined : { connection_id: connection.id, database: tab.database ?? null, sql: rowsResult.sql }}
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
