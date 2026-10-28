/** "Run on connections": the editor's script (or its selection) on each connection chosen,
 *  a few at a time, each in a session of its own. The dialog shows how each run went; the
 *  last result with rows of each connection is copied into an in-memory DuckDB, and a new tab
 *  queries them together (see `multiRun.ts`). */
import { useEffect, useMemo, useRef, useState } from "react";
import { api, type QueryEvent, type ResultTable, type SavedConnection, toCommandError } from "../api";
import { typeBadge } from "../connections";
import { withDriver } from "../drivers/DriverPopup";
import { DRIVER_NAMES } from "../drivers/names";
import { formatCount, msg, plural, t } from "../i18n";
import { ask } from "../ui/ask";
import { workbench } from "../workbench";
import { type Kept, type Outcome, type ScriptProblem, mergedSql, planScript, pool, tableOf } from "./multiRun";

/** Connections run at the same time. */
const PARALLEL = 4;

interface Props {
  /** The SQL to run, as the editor holds it. */
  script: string;
  /** The tab's connection: chosen at first, and the dialect the script is checked in. */
  origin: SavedConnection;
  onClose: () => void;
}

/** What a statement left: its results with rows (a statement can have several), the rows it
 *  changed, or why it failed. */
interface Ran {
  results: { jobId: number; columns: string[]; rows: number }[];
  affected: number;
  error: string | null;
}

const PROBLEMS: Record<ScriptProblem, string> = {
  empty: msg("There is no statement to run."),
  parameters: msg("Parameters (:name, ?, ${name}) are not filled in on several connections: write the values in the script."),
  commands: msg("Client commands (@set, @include, @export…) run only in the tab."),
};

/** Runs `sql` on the session and waits for its end; its jobs stay open. */
function runStatement(sessionId: number, sql: string, onJob: (jobId: number) => void): Promise<Ran> {
  return new Promise((resolve) => {
    const ran: Ran = { results: [], affected: 0, error: null };
    /** The job the next events are about; the first one's id comes with the answer. */
    let current: { jobId: number | null; columns: string[] | null; rows: number } = { jobId: null, columns: null, rows: 0 };
    const all = [current];
    let ended = false;
    let known = false;
    const settle = () => {
      if (!ended || !known) return;
      for (const r of all) if (r.jobId !== null && r.columns !== null) ran.results.push({ jobId: r.jobId, columns: r.columns, rows: r.rows });
      resolve(ran);
    };
    const onEvent = (e: QueryEvent) => {
      switch (e.type) {
        case "started":
          current.columns = e.columns.map((c) => c.name);
          break;
        case "finished":
          current.rows = e.rows;
          if (!e.more) ended = true;
          break;
        case "affected":
          ran.affected += e.rows;
          ended = true;
          break;
        case "failed":
          ran.error = e.error.message;
          ended = true;
          break;
        case "next":
          current = { jobId: e.job_id, columns: null, rows: 0 };
          all.push(current);
          onJob(e.job_id);
          break;
        default:
          return;
      }
      settle();
    };
    api.runQuery(sessionId, sql, null, onEvent).then(
      (jobId) => {
        all[0]!.jobId = jobId;
        onJob(jobId);
        known = true;
        settle();
      },
      (e: unknown) => {
        ran.error = toCommandError(e).message;
        ended = known = true;
        settle();
      },
    );
  });
}

export function MultiRunDialog({ script, origin, onClose }: Props) {
  const connections = useMemo(() => workbench().connections(), []);
  const [chosen, setChosen] = useState<Set<string>>(() => new Set([origin.id]));
  const [filter, setFilter] = useState("");
  const [outcomes, setOutcomes] = useState<Map<string, Outcome> | null>(null);
  const [phase, setPhase] = useState<"choose" | "running" | "merging" | "done">("choose");
  const [problem, setProblem] = useState<string | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  const stopped = useRef(false);
  /** The job each connection runs now, to cancel it. */
  const running = useRef(new Map<string, number>());
  /** Sessions and jobs still open, freed when the dialog goes. */
  const sessions = useRef(new Set<number>());
  const jobs = useRef(new Set<number>());

  useEffect(
    () => () => {
      stopped.current = true;
      for (const j of jobs.current) void api.closeQuery(j).catch(() => undefined);
      for (const s of sessions.current) void api.closeSession(s).catch(() => undefined);
    },
    [],
  );

  const shown = connections.filter((c) => {
    const f = filter.trim().toLowerCase();
    return !f || c.name.toLowerCase().includes(f) || c.host.toLowerCase().includes(f) || DRIVER_NAMES[c.driver].toLowerCase().includes(f);
  });
  const sameDriver = shown.filter((c) => c.driver === origin.driver);
  const targets = connections.filter((c) => chosen.has(c.id));
  const toggle = (id: string) =>
    setChosen((all) => {
      const next = new Set(all);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const setOutcome = (id: string, o: Outcome) => setOutcomes((all) => new Map(all).set(id, o));
  const free = (jobId: number) => {
    jobs.current.delete(jobId);
    void api.closeQuery(jobId).catch(() => undefined);
  };

  async function start() {
    setProblem(null);
    const checked = planScript(script, await api.analyzeSql(script, origin.driver, 0));
    if (typeof checked === "string") return setProblem(t(PROBLEMS[checked]));
    if (checked.writes) {
      const prod = targets.filter((c) => c.connection_type === "prod").map((c) => c.name);
      const message =
        t("This script may change data on {count}.", { count: plural(targets.length, "{n} connection", "{n} connections") }) +
        (prod.length ? ` ${t("Production: {names}.", { names: prod.join(", ") })}` : "");
      if (!(await ask(message, { confirm: t("Run"), danger: prod.length > 0 }))) return;
    }
    stopped.current = false;
    setOutcomes(new Map(targets.map((c) => [c.id, { state: "queued" }])));
    setPhase("running");
    const kept = new Map<string, { jobId: number; columns: string[] }>();
    await pool(targets, PARALLEL, async (c) => {
      const result = await runOn(c, checked.writes);
      if (result) kept.set(c.id, result);
    }, () => stopped.current);
    if (stopped.current) {
      setOutcomes((all) => {
        const next = new Map(all);
        for (const [id, o] of next) if (o.state === "queued") next.set(id, { state: "skipped", reason: t("Stopped") });
        return next;
      });
    }
    await merge(targets.filter((c) => kept.has(c.id)).map((c) => ({ c, ...kept.get(c.id)! })));
  }

  /** Runs the script on `c`; its last result with rows, loaded in full. */
  async function runOn(c: SavedConnection, writes: boolean): Promise<{ jobId: number; columns: string[] } | null> {
    if (writes && c.read_only) {
      setOutcome(c.id, { state: "skipped", reason: t("Read-only connection") });
      return null;
    }
    setOutcome(c.id, { state: "running", statement: 0 });
    let sessionId: number;
    try {
      sessionId = (await api.openSession(c.id)).session_id;
    } catch (e) {
      setOutcome(c.id, { state: "failed", statement: 0, message: toCommandError(e).message });
      return null;
    }
    sessions.current.add(sessionId);
    let last: { jobId: number; columns: string[]; rows: number } | null = null;
    try {
      // Checked again in the connection's own dialect: batches and quoting differ.
      const plan = planScript(script, await api.analyzeSql(script, c.driver, 0));
      if (typeof plan === "string") throw new Error(t(PROBLEMS[plan]));
      let affected = 0;
      for (const [i, sql] of plan.statements.entries()) {
        if (stopped.current) {
          setOutcome(c.id, { state: "skipped", reason: t("Stopped") });
          if (last) free(last.jobId);
          return null;
        }
        setOutcome(c.id, { state: "running", statement: i + 1 });
        const started: number[] = [];
        const ran = await runStatement(sessionId, sql, (jobId) => {
          jobs.current.add(jobId);
          started.push(jobId);
          running.current.set(c.id, jobId);
        });
        running.current.delete(c.id);
        affected += ran.affected;
        // The last result with rows is the one kept; the others go.
        for (const r of ran.results) {
          if (last) free(last.jobId);
          last = r;
        }
        for (const j of started) if (!ran.results.some((r) => r.jobId === j)) free(j);
        if (ran.error !== null) {
          setOutcome(c.id, { state: "failed", statement: i + 1, message: ran.error });
          if (last) free(last.jobId);
          return null;
        }
        setOutcome(c.id, { state: "done", statements: i + 1, rows: last?.rows ?? null, affected });
      }
      if (!last) return null;
      // Every row, before the session goes: the copy reads what the job holds.
      await api.fetchWhole(last.jobId, () => undefined);
      return { jobId: last.jobId, columns: last.columns };
    } catch (e) {
      setOutcome(c.id, { state: "failed", statement: 0, message: toCommandError(e).message });
      if (last) free(last.jobId);
      return null;
    } finally {
      sessions.current.delete(sessionId);
      void api.closeSession(sessionId).catch(() => undefined);
    }
  }

  /** Copies the kept results into an in-memory DuckDB and opens a tab on them together. */
  async function merge(kept: { c: SavedConnection; jobId: number; columns: string[] }[]) {
    if (kept.length === 0) {
      setSummary(t("No connection returned rows."));
      setPhase("done");
      return;
    }
    setPhase("merging");
    const tables: ResultTable[] = kept.map((k, i) => ({ job_id: k.jobId, table: tableOf(i) }));
    const merged: Kept[] = kept.map((k, i) => ({ name: k.c.name, table: tableOf(i), columns: k.columns }));
    try {
      const opened = await withDriver(() =>
        api.openResultDb(t("Results of {count}", { count: plural(kept.length, "{n} connection", "{n} connections") }), tables),
      );
      workbench().openTemporaryTab(
        "query",
        { ...opened.connection, read_only: true },
        { sql: mergedSql(merged), title: t("Results of {count}", { count: plural(kept.length, "{n} connection", "{n} connections") }), autorun: true },
        () => void api.closeResultDb(opened.connection.id),
      );
      setSummary(t("The results opened in a new tab."));
    } catch (e) {
      setSummary(toCommandError(e).message);
    } finally {
      for (const k of kept) free(k.jobId);
      setPhase("done");
    }
  }

  function stop() {
    stopped.current = true;
    for (const jobId of running.current.values()) void api.cancelQuery(jobId).catch(() => undefined);
  }

  const busy = phase === "running" || phase === "merging";
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="modal multi-run" onKeyDown={(e) => e.key === "Escape" && !busy && onClose()}>
        <h2>{t("Run on connections")}</h2>
        {outcomes === null ? (
          <>
            <p className="muted small">
              {t("Runs the script on each connection chosen, {n} at a time, in a session of its own. The results come together in a new tab, with the name of the connection of each row.", { n: PARALLEL })}
            </p>
            <div className="row tight">
              <input
                className="multi-run-filter"
                placeholder={t("Filter connections")}
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                autoFocus
              />
              <button type="button" onClick={() => setChosen(new Set([...chosen, ...sameDriver.map((c) => c.id)]))} disabled={sameDriver.length === 0}>
                {t("All {driver}", { driver: DRIVER_NAMES[origin.driver] })}
              </button>
              <button type="button" onClick={() => setChosen(new Set())} disabled={chosen.size === 0}>
                {t("None")}
              </button>
            </div>
            <div className="multi-run-list" role="list">
              {shown.length === 0 && <p className="muted small">{t("No connection matches.")}</p>}
              {shown.map((c) => (
                <label key={c.id} className="multi-run-row check" role="listitem" data-id={c.id}>
                  <input type="checkbox" checked={chosen.has(c.id)} onChange={() => toggle(c.id)} />
                  {c.color && <span className="dot" style={{ background: c.color }} />}
                  <span className="multi-run-name">{c.name}</span>
                  {typeBadge(c) && <span className={`type-badge ${c.connection_type}`}>{typeBadge(c)}</span>}
                  <span className="spacer" />
                  <span className="muted small">{DRIVER_NAMES[c.driver]}</span>
                </label>
              ))}
            </div>
            {problem && <p className="notice error">{problem}</p>}
          </>
        ) : (
          <div className="multi-run-list" role="list">
            {targets.map((c) => (
              <OutcomeRow key={c.id} connection={c} outcome={outcomes.get(c.id) ?? { state: "queued" }} />
            ))}
          </div>
        )}
        {summary && <p className="notice">{summary}</p>}
        <div className="actions">
          {outcomes === null && <span className="muted small">{plural(chosen.size, "{n} connection chosen", "{n} connections chosen")}</span>}
          <span className="spacer" />
          {phase === "choose" && (
            <button type="button" className="primary" data-action="run" onClick={() => void start()} disabled={chosen.size === 0}>
              {t("Run")}
            </button>
          )}
          {busy && (
            <button type="button" data-action="stop" onClick={stop} disabled={phase === "merging" || stopped.current}>
              {t("Stop")}
            </button>
          )}
          <button type="button" onClick={onClose} disabled={busy}>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}

function OutcomeRow({ connection, outcome }: { connection: SavedConnection; outcome: Outcome }) {
  let text: string;
  switch (outcome.state) {
    case "queued":
      text = t("Waiting");
      break;
    case "running":
      text = outcome.statement === 0 ? t("Connecting…") : t("Statement {n}…", { n: outcome.statement });
      break;
    case "done": {
      const parts = [plural(outcome.statements, "{n} statement", "{n} statements")];
      if (outcome.rows !== null) parts.push(plural(outcome.rows, "{n} row", "{n} rows"));
      if (outcome.affected > 0) parts.push(t("{n} changed", { n: formatCount(outcome.affected) }));
      text = parts.join(" · ");
      break;
    }
    case "failed":
      text = outcome.statement === 0 ? outcome.message : t("Statement {n}: {message}", { n: outcome.statement, message: outcome.message });
      break;
    case "skipped":
      text = outcome.reason;
      break;
  }
  return (
    <div className={`multi-run-row outcome ${outcome.state}`} role="listitem" data-id={connection.id} data-state={outcome.state}>
      <span className="multi-run-state" aria-hidden />
      <span className="multi-run-name">{connection.name}</span>
      <span className="multi-run-outcome" title={text}>
        {text}
      </span>
    </div>
  );
}
