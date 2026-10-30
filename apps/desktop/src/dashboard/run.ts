/**
 * Running a dashboard's widgets: each reads its script, checks that it only reads, and runs
 * it on a session of its own, so that the widgets of a dashboard run in parallel and one
 * that is slow or fails does not hold up the others.
 */
import { api, type SavedConnection, type ScriptList, toCommandError } from "../api";
import { withEvent } from "../analysis/sqlField";
import type { Result } from "../components/ResultPanel";
import { msg, t } from "../i18n";
import { scriptFile } from "../scripts/scripts";
import { workbench } from "../workbench";
import { connectionOf, findScript, type Refusal, refusal, rowLimit, valueText, type Widget } from "./model";

/** What a widget's last run left. */
export interface WidgetRun {
  running: boolean;
  /** Why it did not run, or failed. */
  error: string | null;
  /** Table and chart: the rows, still held by their job. */
  result: Result | null;
  /** Single value: the first cell of the first row, `null` for a script with no rows. */
  value: string | null;
  /** When the run ended, in milliseconds since the epoch. */
  at: number | null;
}

export const idle: WidgetRun = { running: false, error: null, result: null, value: null, at: null };

/** What went wrong, as the widget says it. */
export const reason = (e: unknown) => (e instanceof Error ? e.message : toCommandError(e).message);

const REFUSALS: Record<Refusal, string> = {
  empty: msg("The script has no statement."),
  changes: msg("The script may change data: a dashboard only runs statements that read."),
  command: msg("Client commands (@set, @include…) do not run in a dashboard."),
  parameters: msg("The script has parameters: a dashboard cannot ask for their values."),
};

/** The sessions of the widgets, one each, opened when first needed. */
export class WidgetSessions {
  private sessions = new Map<string, { connection: string; session: Promise<number | null> }>();
  // Opening may ask for a password: one connection at a time, so that it is asked once.
  private opening = new Map<string, Promise<unknown>>();

  /** The session of widget `id` on `connection`, a new one if it was on another. */
  get(id: string, connection: SavedConnection): Promise<number | null> {
    const had = this.sessions.get(id);
    if (had?.connection === connection.id) return had.session;
    this.close(id);
    const after = this.opening.get(connection.id) ?? Promise.resolve();
    const session = after.then(() => workbench().openSession(connection, true));
    this.opening.set(connection.id, session.catch(() => null));
    this.sessions.set(id, { connection: connection.id, session });
    // A cancelled or failed attempt is made again at the next run.
    void session.then(
      (sid) => sid === null && this.sessions.get(id)?.session === session && this.sessions.delete(id),
      () => this.sessions.get(id)?.session === session && this.sessions.delete(id),
    );
    return session;
  }

  close(id: string) {
    const had = this.sessions.get(id);
    this.sessions.delete(id);
    void had?.session.then((sid) => sid !== null && workbench().closeSession(sid)).catch(() => undefined);
  }

  closeAll() {
    for (const id of [...this.sessions.keys()]) this.close(id);
  }
}

/** Runs `sql` on `sid` until its rows are in, it fails or it ends without rows. `update`
 *  shows the result as it goes; `limit` caps the rows read. */
function runStatement(sid: number, sql: string, limit: number, update: (result: Result) => void): Promise<Result> {
  return new Promise((resolve) => {
    let result: Result = { id: 0, sql, offset: 0, jobId: null, columns: null, rows: 0, fetching: false, status: { kind: "running" }, limit };
    const show = (next: Result) => {
      result = next;
      update(result);
      const s = result.status;
      // Ended: its rows are all in (the job known, which an early event may precede), or it failed.
      if (s.kind === "error" || s.kind === "affected" || (result.jobId !== null && s.kind === "rows" && s.finished)) resolve(result);
    };
    show(result);
    api.runQuery(sid, sql, limit, (event) => show(withEvent(result, event)), false).then(
      (jobId) => show({ ...result, jobId }),
      (e: unknown) => show({ ...result, status: { kind: "error", error: toCommandError(e) } }),
    );
  });
}

/** Everything a run needs from the dashboard around the widget. */
export interface RunContext {
  project: string | null;
  /** The dashboard's own connection. */
  connection: string | null;
  scripts: Promise<ScriptList>;
  sessions: WidgetSessions;
  /** False once a newer run of the widget, or the end of the tab, has replaced this one. */
  current: () => boolean;
  update: (patch: Partial<WidgetRun>) => void;
  /** The job of the rows the widget showed before, to release. */
  previous: number | null;
}

/** Looks the widget's script up and reads it: its text, and the connection to run it on. */
async function load(widget: Widget, ctx: RunContext): Promise<{ sql: string; connection: SavedConnection }> {
  const list = await ctx.scripts;
  const entry = findScript(list.entries, widget.script);
  if (!entry) throw new Error(t("The script {path} no longer exists: it was renamed or deleted.", { path: widget.script }));
  const id = connectionOf(widget, entry, ctx.connection);
  const connection = workbench().connections().find((c) => c.id === id);
  if (!connection) throw new Error(t("The connection of this widget no longer exists."));
  const sql = await api.readTextFile(scriptFile(list.root, entry.path)).catch(() => {
    throw new Error(t("The script {path} could not be read.", { path: widget.script }));
  });
  return { sql, connection };
}

/** Runs `widget`'s script and reports through `ctx.update`; never throws. */
export async function runWidget(widget: Widget, ctx: RunContext): Promise<void> {
  if (ctx.previous !== null) void api.closeQuery(ctx.previous);
  ctx.update({ running: true, error: null, result: null, value: null });
  const started: number[] = [];
  const settle = (patch: Partial<WidgetRun>) => {
    if (ctx.current()) ctx.update({ running: false, at: Date.now(), ...patch });
  };
  try {
    const { sql, connection } = await load(widget, ctx);
    const statements = (await api.analyzeSql(sql, connection.driver, 0)).statements;
    const refused = refusal(statements);
    if (refused) throw new Error(t(REFUSALS[refused]));
    const sid = await ctx.sessions.get(widget.id, connection);
    if (sid === null) throw new Error(t("Could not connect to {name}.", { name: connection.name }));
    let last: Result | null = null;
    for (const s of statements) {
      // The widget shows the last statement: the rows of the earlier ones go.
      if (last?.jobId != null) void api.closeQuery(last.jobId);
      last = await runStatement(sid, sql.slice(s.start, s.end), rowLimit(widget), (result) => {
        if (result.jobId !== null && !started.includes(result.jobId)) started.push(result.jobId);
        // Table and chart show the rows as they arrive.
        if (ctx.current() && widget.view !== "value" && s === statements.at(-1)) ctx.update({ result });
      });
      if (last.status.kind !== "rows") break;
    }
    if (!last || !ctx.current()) {
      for (const job of started) void api.closeQuery(job);
      return;
    }
    if (last.status.kind === "error") throw new Error(last.status.error.message);
    if (last.status.kind !== "rows") throw new Error(t("The script returned no rows to show."));
    if (widget.view !== "value") {
      settle({ result: last });
      return;
    }
    const page = last.jobId === null || last.rows === 0 ? null : await api.fetchRows(last.jobId, 0, 1);
    for (const job of started) void api.closeQuery(job);
    settle({ result: null, value: page ? valueText(page.columns[0]?.[0]) : null });
  } catch (e) {
    for (const job of started) void api.closeQuery(job);
    settle({ result: null, error: reason(e) });
  }
}
