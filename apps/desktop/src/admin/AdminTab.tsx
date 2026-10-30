import { type ReactNode, useEffect, useRef, useState } from "react";
import {
  api,
  type Capabilities,
  type EndSession,
  type Maintenance,
  type ObjectStatistics,
  type ServerLock,
  type ServerParameter,
  type ServerQuery,
  type ServerSession,
  toCommandError,
} from "../api";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { ask } from "../ui/ask";
import { Icon } from "../ui/Icon";
import { msg, t } from "../i18n";
import { Dashboard } from "./Dashboard";
import { pushSample, type Sample } from "./dashboardModel";
import { Parameters } from "./Parameters";
import { type Activity, activity, blockingChains, type Chain, matches, seconds } from "./sessions";
import { ObjectsView, TopQueries } from "./Statistics";
import { Users } from "./Users";
import { EmptyState, LoadingState } from "../ui/State";

export type AdminView = "sessions" | "locks" | "statements" | "objects" | "parameters" | "dashboard" | "users";

/** Views read on demand, not every few seconds: the server sums their numbers over hours, or
 *  they do not change while the tab is open. */
const ON_DEMAND = new Set<AdminView>(["statements", "objects", "parameters"]);

/** The label of each view's button. */
const VIEW_LABELS: Record<AdminView, string> = {
  sessions: msg("Sessions"),
  locks: msg("Locks"),
  dashboard: msg("Dashboard"),
  users: msg("Users and roles"),
  statements: msg("Costliest statements"),
  objects: msg("Tables and indexes"),
  parameters: msg("Parameters"),
};

/** The views `caps` offers, in the order of the tab's buttons: a database without sessions
 *  (SQLite, DuckDB) still lists its parameters. */
export function availableViews(caps: Capabilities): AdminView[] {
  const views: AdminView[] = [];
  if (caps.server_sessions) views.push("sessions", "locks");
  if (caps.server_metrics) views.push("dashboard");
  if (caps.security) views.push("users");
  if (caps.server_statistics) views.push("statements", "objects");
  if (caps.server_parameters) views.push("parameters");
  return views;
}

/** The server's sessions and locks, read again every few seconds; its costliest statements
 *  and the use of its tables and indexes. */
export interface AdminTabModel extends TabBase {
  kind: "admin";
  view?: AdminView;
  /** Seconds between refreshes; 0 refreshes only on demand. */
  interval?: number;
}

declare module "../tabs/registry" {
  interface TabKinds {
    admin: AdminTabModel;
  }
}

export const INTERVALS: [number, string][] = [
  [0, msg("Manual")],
  [2, msg("Every 2 s")],
  [5, msg("Every 5 s")],
  [10, msg("Every 10 s")],
  [30, msg("Every 30 s")],
];

/** The badge of each activity, and the count that filters the list by it. */
const ACTIVITIES: [Activity, string][] = [
  ["waiting", msg("waiting")],
  ["running", msg("running")],
  ["transaction", msg("idle in transaction")],
  ["idle", msg("idle")],
];

const label = (a: Activity) => t(ACTIVITIES.find(([k]) => k === a)?.[1] ?? a);

interface Snapshot {
  sessions: ServerSession[];
  /** `null` until the Locks view asks, or when the server would not tell. */
  locks: ServerLock[] | null;
  locksError: string | null;
  at: Date;
}

/** The statements or the tables and indexes, as last read, or why they could not be. */
interface Statistics {
  queries: ServerQuery[] | null;
  objects: ObjectStatistics[] | null;
  error: string | null;
}

export function AdminTab({ tab, connection, active, sideSession, onChange }: TabProps<AdminTabModel>) {
  const [caps, setCaps] = useState<Capabilities | null>(null);
  // A view the database does not offer gives way to the first it does.
  const offered = caps ? availableViews(caps) : [];
  const requested = tab.view ?? "sessions";
  const view = (!offered.includes(requested) && offered[0]) || requested;
  const interval = tab.interval ?? 5;
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [only, setOnly] = useState<Activity | null>(null);
  const [onlyWaiting, setOnlyWaiting] = useState(false);
  const [version, setVersion] = useState(0);
  const [statistics, setStatistics] = useState<Statistics>({ queries: null, objects: null, error: null });
  const [parameters, setParameters] = useState<{ list: ServerParameter[] | null; error: string | null }>({
    list: null,
    error: null,
  });
  /** The dashboard's last snapshots, kept across views so that its sparklines have a past. */
  const [samples, setSamples] = useState<Sample[]>([]);
  const busy = useRef(false);
  /** A load asked while one ran: it runs once that one ends, with the view then shown. */
  const again = useRef(false);
  const viewNow = useRef(view);
  viewNow.current = view;

  // Reads the lists on the navigator's session: it stays in autocommit, where PostgreSQL's
  // activity is fresh at each statement, not frozen for a transaction.
  async function load() {
    if (busy.current) {
      again.current = true;
      return;
    }
    busy.current = true;
    setLoading(true);
    try {
      do {
        again.current = false;
        const sid = await sideSession();
        if (sid === null) return;
        const capabilities = workbench().capabilities(sid) ?? null;
        setCaps(capabilities);
        if (viewNow.current === "dashboard") {
          await loadMetrics(sid, capabilities);
          continue;
        }
        if (viewNow.current === "parameters") {
          await loadParameters(sid, capabilities);
          continue;
        }
        // The users and roles read themselves.
        if (viewNow.current === "users") return;
        if (ON_DEMAND.has(viewNow.current)) {
          await loadStatistics(sid, viewNow.current, capabilities);
          continue;
        }
        if (!capabilities?.server_sessions) return;
        const sessions = await api.serverSessions(sid);
        let locks: ServerLock[] | null = null;
        let locksError: string | null = null;
        if (viewNow.current === "locks") {
          try {
            locks = await api.serverLocks(sid);
          } catch (e) {
            locksError = toCommandError(e).message;
          }
        }
        setSnapshot({ sessions, locks, locksError, at: new Date() });
        setError(null);
      } while (again.current);
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      busy.current = false;
      setLoading(false);
    }
  }

  /** Reads the statements or the objects of `view`; an error is the view's, not the tab's. */
  async function loadStatistics(sid: number, view: AdminView, capabilities: Capabilities | null) {
    if (!capabilities?.server_statistics) return;
    try {
      if (view === "statements") {
        const queries = await api.topQueries(sid);
        setStatistics((s) => ({ ...s, queries, error: null }));
      } else {
        const objects = await api.objectStatistics(sid);
        setStatistics((s) => ({ ...s, objects, error: null }));
      }
      setSnapshot((s) => s && { ...s, at: new Date() });
    } catch (e) {
      setStatistics((s) => ({ ...s, error: toCommandError(e).message }));
    }
  }

  /** Reads a snapshot for the dashboard; its rates come from the ones before. */
  async function loadMetrics(sid: number, capabilities: Capabilities | null) {
    if (!capabilities?.server_metrics) return;
    try {
      const metrics = await api.serverMetrics(sid);
      setSamples((s) => pushSample(s, { at: Date.now(), metrics }));
      setSnapshot((s) => ({ sessions: s?.sessions ?? [], locks: s?.locks ?? null, locksError: null, at: new Date() }));
      setError(null);
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  /** Reads the parameters; an error is the view's, not the tab's. */
  async function loadParameters(sid: number, capabilities: Capabilities | null) {
    if (!capabilities?.server_parameters) return;
    try {
      setParameters({ list: await api.serverParameters(sid), error: null });
      setSnapshot((s) => s && { ...s, at: new Date() });
    } catch (e) {
      setParameters((p) => ({ ...p, error: toCommandError(e).message }));
    }
  }

  async function maintain(schema: string, table: string, operation: Maintenance): Promise<string[]> {
    const sid = await sideSession();
    if (sid === null) return [];
    const lines = await api.maintainTable(sid, schema, table, operation);
    // The numbers changed: read them again.
    setVersion((v) => v + 1);
    return lines;
  }

  useEffect(() => {
    if (!active) return;
    void load();
    if (interval <= 0 || ON_DEMAND.has(view) || view === "users") return;
    const timer = window.setInterval(() => void load(), interval * 1000);
    return () => window.clearInterval(timer);
  }, [active, interval, view, version]);

  async function end(session: ServerSession, how: EndSession) {
    const question =
      how === "cancel"
        ? t("Cancel the statement of session {id}? The session stays open.", { id: session.id })
        : t("End session {id} of {user}? Its open transaction is rolled back.", {
            id: session.id,
            user: session.user ?? t("unknown user"),
          });
    const confirmed = await ask(question, {
      confirm: t(how === "cancel" ? "Cancel statement" : "End session"),
      danger: true,
    });
    if (!confirmed) return;
    try {
      const sid = await sideSession();
      if (sid === null) return;
      await api.endServerSession(sid, session.id, how);
      setVersion((v) => v + 1);
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  const sessions = snapshot?.sessions ?? [];
  const counts = new Map<Activity, number>();
  for (const s of sessions) counts.set(activity(s), (counts.get(activity(s)) ?? 0) + 1);
  const shown = sessions.filter((s) => (only === null || activity(s) === only) && matches(s, filter));
  const chosen = sessions.find((s) => s.id === selected) ?? null;
  const waiting = counts.get("waiting") ?? 0;
  const unsupported = caps !== null && offered.length === 0;
  const show = (id: string) => {
    setSelected(id);
    onChange({ view: "sessions" });
  };

  return (
    <div className="object-tab admin-tab" style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <div className="object-title">
          <strong>{t("Server activity")}</strong>
          <span className="muted small">{connection.name}</span>
        </div>
        <span className="spacer" />
        {snapshot && (
          <span className="muted small" title={t("When the lists were read")}>
            {snapshot.at.toLocaleTimeString()}
          </span>
        )}
        <select
          className="admin-interval"
          value={interval}
          onChange={(e) => onChange({ interval: Number(e.target.value) })}
          aria-label={t("Refresh")}
        >
          {INTERVALS.map(([value, text]) => (
            <option key={value} value={value}>
              {t(text)}
            </option>
          ))}
        </select>
        <button
          className="square"
          disabled={loading}
          onClick={() => setVersion((v) => v + 1)}
          title={t("Refresh now")}
          aria-label={t("Refresh now")}
        >
          <Icon name="refresh" />
        </button>
      </header>
      <nav className="object-sections">
        {offered.map((v) => (
          <button key={v} className={view === v ? "active" : ""} onClick={() => onChange({ view: v })}>
            {t(VIEW_LABELS[v])}
            {v === "sessions" && <span className="count">{sessions.length}</span>}
            {v === "locks" && waiting > 0 && (
              <span className="count admin-blocked-count" title={t("{n} waiting for a lock", { n: waiting })}>
                {waiting}
              </span>
            )}
          </button>
        ))}
      </nav>
      {unsupported && (
        <div className="object-body">
          <div className="notice small">{t("This database does not list the sessions of its server.")}</div>
        </div>
      )}
      {error && <div className="notice error small admin-error">{error}</div>}
      {!unsupported && view === "dashboard" && (
        <div className="object-body admin-body">
          <Dashboard samples={samples} />
        </div>
      )}
      {!unsupported && view === "parameters" && (
        <div className="object-body admin-body">
          {parameters.error && <div className="notice error small" data-field="parameters-error">{parameters.error}</div>}
          {parameters.list && (
            <Parameters
              parameters={parameters.list}
              onOpen={(sql) => workbench().openTab("query", connection.id, { sql })}
            />
          )}
          {!parameters.error && !parameters.list && (
            loading && <LoadingState compact title={t("Reading the parameters…")} />
          )}
        </div>
      )}
      {!unsupported && view === "sessions" && (
        <div className="object-body admin-body">
          <div className="admin-toolbar">
            <div className="admin-filters" role="group" aria-label={t("Show")}>
              <button className={only === null ? "admin-filter active" : "admin-filter"} onClick={() => setOnly(null)}>
                <strong>{sessions.length}</strong> {t("all")}
              </button>
              {ACTIVITIES.map(([a, text]) => (
                <button
                  key={a}
                  className={`admin-filter ${a}${only === a ? " active" : ""}`}
                  disabled={!counts.get(a) && only !== a}
                  onClick={() => setOnly(only === a ? null : a)}
                >
                  <span className="admin-dot" />
                  <strong>{counts.get(a) ?? 0}</strong> {t(text)}
                </button>
              ))}
            </div>
            <span className="spacer" />
            <input
              type="search"
              placeholder={t("Filter by user, database, client or statement")}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>
          <div className="admin-split">
            <SessionTable sessions={shown} selected={selected} onSelect={setSelected} />
            <SessionDetails
              session={chosen}
              caps={caps}
              readOnly={connection.read_only}
              onSelect={setSelected}
              onEnd={(s, how) => void end(s, how)}
              onOpen={(sql) => workbench().openTab("query", connection.id, { sql })}
            />
          </div>
        </div>
      )}
      {ON_DEMAND.has(view) && (
        <div className="object-body admin-body">
          {statistics.error && <div className="notice error small" data-field="statistics-error">{statistics.error}</div>}
          {view === "statements" && statistics.queries && (
            <TopQueries
              queries={statistics.queries}
              onOpen={(sql) => workbench().openTab("query", connection.id, { sql })}
              onExplain={
                caps?.explain ? (sql) => workbench().openTab("explain", connection.id, { sql, analyze: false }) : null
              }
            />
          )}
          {view === "objects" && statistics.objects && (
            <ObjectsView
              objects={statistics.objects}
              maintenance={caps?.maintenance ?? []}
              readOnly={connection.read_only}
              onMaintain={maintain}
            />
          )}
          {!statistics.error && (view === "statements" ? !statistics.queries : !statistics.objects) && (
            loading && <LoadingState compact title={t("Reading the statistics…")} />
          )}
        </div>
      )}
      {view === "users" && caps?.security && (
        <Users connection={connection} active={active} refresh={version} sideSession={sideSession} />
      )}
      {!unsupported && view === "locks" && (
        <div className="object-body admin-body">
          <Locks
            sessions={sessions}
            locks={snapshot?.locks ?? null}
            locksError={snapshot?.locksError ?? null}
            onlyWaiting={onlyWaiting}
            onOnlyWaiting={setOnlyWaiting}
            onSelect={show}
          />
        </div>
      )}
    </div>
  );
}

function Badge({ session }: { session: ServerSession }) {
  const a = activity(session);
  return (
    <span className={`admin-badge ${a}`} title={session.state ?? undefined}>
      <span className="admin-dot" />
      {label(a)}
    </span>
  );
}

function SessionTable({
  sessions,
  selected,
  onSelect,
}: {
  sessions: ServerSession[];
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="admin-list">
      <table className="meta-table admin-table">
        <thead>
          <tr>
            <th className="num">{t("Id")}</th>
            <th>{t("User")}</th>
            <th>{t("Database")}</th>
            <th>{t("State")}</th>
            <th className="num">{t("Running for")}</th>
            <th>{t("Statement")}</th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((s) => (
            <tr
              key={s.id}
              data-id={s.id}
              className={["admin-session", s.id === selected && "selected", activity(s), s.current && "current"]
                .filter(Boolean)
                .join(" ")}
              onClick={() => onSelect(s.id)}
            >
              <td className="num">{s.id}</td>
              <td>
                {s.user}
                {s.current && (
                  <span className="admin-own" title={t("The session of this app that reads the list")}>
                    {t("this app")}
                  </span>
                )}
              </td>
              <td>{s.database}</td>
              <td>
                <Badge session={s} />
              </td>
              <td className="num">{seconds(s.query_seconds)}</td>
              <td className="admin-query" title={s.query ?? undefined}>
                {s.query}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {sessions.length === 0 && <EmptyState compact className="admin-empty" title={t("No sessions match.")} />}
    </div>
  );
}

function SessionDetails({
  session,
  caps,
  readOnly,
  onSelect,
  onEnd,
  onOpen,
}: {
  session: ServerSession | null;
  caps: Capabilities | null;
  readOnly: boolean;
  onSelect: (id: string) => void;
  onEnd: (session: ServerSession, how: EndSession) => void;
  onOpen: (sql: string) => void;
}) {
  if (!session) {
    return (
      <aside className="admin-details empty">
        <p className="muted small">{t("Select a session to see its statement and end it.")}</p>
      </aside>
    );
  }
  const running = session.query_seconds !== null;
  const why = readOnly
    ? t("The connection is read-only")
    : session.current
      ? t("This is the session of this app")
      : undefined;
  const fields: [string, string, ReactNode][] = [
    ["user", t("User"), session.user],
    ["database", t("Database"), session.database],
    ["client", t("Client"), session.client],
    ["application", t("Application"), session.application],
    ["state", t("State"), session.state],
    ["since", t("Since"), session.started],
    ["running", t("Running for"), seconds(session.query_seconds)],
    ["wait", t("Waits for"), session.wait],
  ];
  return (
    <aside className="admin-details">
      <div className="admin-details-title">
        <strong>{t("Session {id}", { id: session.id })}</strong>
        <Badge session={session} />
      </div>
      <dl className="admin-fields">
        {fields
          .filter(([, , value]) => value)
          .map(([key, name, value]) => (
            <div key={key} data-field={key}>
              <dt>{name}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        {session.blocked_by.length > 0 && (
          <div className="admin-blocked-by" data-field="blocked-by">
            <dt>{t("Blocked by")}</dt>
            <dd>
              {session.blocked_by.map((id) => (
                <button key={id} className="link" onClick={() => onSelect(id)}>
                  {id}
                </button>
              ))}
            </dd>
          </div>
        )}
      </dl>
      <pre className="admin-statement">{session.query ?? t("No statement")}</pre>
      <div className="admin-actions">
        <button disabled={!session.query} onClick={() => session.query && onOpen(session.query)}>
          {t("Open in editor")}
        </button>
        {caps?.cancel_server_queries && (
          <button
            disabled={!running || !!why}
            title={why ?? (running ? undefined : t("It runs no statement"))}
            onClick={() => onEnd(session, "cancel")}
          >
            {t("Cancel statement")}
          </button>
        )}
        <button className="admin-end" disabled={!!why} title={why} onClick={() => onEnd(session, "terminate")}>
          {t("End session")}
        </button>
      </div>
    </aside>
  );
}

function Locks({
  sessions,
  locks,
  locksError,
  onlyWaiting,
  onOnlyWaiting,
  onSelect,
}: {
  sessions: ServerSession[];
  locks: ServerLock[] | null;
  locksError: string | null;
  onlyWaiting: boolean;
  onOnlyWaiting: (on: boolean) => void;
  onSelect: (id: string) => void;
}) {
  const chains = blockingChains(sessions);
  const shown = (locks ?? []).filter((l) => !onlyWaiting || !l.granted);
  return (
    <div className="admin-list">
      <h3 className="admin-heading">{t("Waiting chains")}</h3>
      {chains.length === 0 ? (
        <p className="muted small">{t("No session waits for another.")}</p>
      ) : (
        <ul className="admin-chains">
          {chains.map((c) => (
            <ChainNode key={c.session.id} chain={c} onSelect={onSelect} />
          ))}
        </ul>
      )}
      <div className="admin-toolbar">
        <h3 className="admin-heading">{t("Locks")}</h3>
        <label className="check small">
          <input type="checkbox" checked={onlyWaiting} onChange={(e) => onOnlyWaiting(e.target.checked)} />
          {t("Only locks waited for")}
        </label>
      </div>
      {locksError && <div className="notice error small">{locksError}</div>}
      {locks && (
        <table className="meta-table admin-table admin-locks">
          <thead>
            <tr>
              <th className="num">{t("Session")}</th>
              <th>{t("Kind")}</th>
              <th>{t("Object")}</th>
              <th>{t("Mode")}</th>
              <th>{t("Status")}</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((l, i) => (
              <tr key={i} className={l.granted ? "" : "waiting"}>
                <td className="num">
                  <button className="link" onClick={() => onSelect(l.session)}>
                    {l.session}
                  </button>
                </td>
                <td>{l.kind}</td>
                <td>{l.object}</td>
                <td>{l.mode}</td>
                <td>
                  <span className={`admin-badge ${l.granted ? "held" : "waiting"}`}>
                    <span className="admin-dot" />
                    {t(l.granted ? "Held" : "Waiting")}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {locks && shown.length === 0 && <p className="muted small">{t("No locks.")}</p>}
    </div>
  );
}

function ChainNode({ chain, onSelect }: { chain: Chain; onSelect: (id: string) => void }) {
  const s = chain.session;
  return (
    <li>
      <button className="admin-chain-session" data-id={s.id} onClick={() => onSelect(s.id)}>
        <span className="admin-chain-id">{s.id}</span>
        <Badge session={s} />
        <span className="muted small">{[s.user, seconds(s.query_seconds)].filter(Boolean).join(" · ")}</span>
        {s.query && <code className="admin-chain-query">{s.query}</code>}
      </button>
      {chain.waiting.length > 0 && (
        <ul>
          {chain.waiting.map((c) => (
            <ChainNode key={c.session.id} chain={c} onSelect={onSelect} />
          ))}
        </ul>
      )}
    </li>
  );
}
