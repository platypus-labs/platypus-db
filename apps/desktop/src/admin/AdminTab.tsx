import { useEffect, useRef, useState } from "react";
import { api, type Capabilities, type EndSession, type ServerLock, type ServerSession, toCommandError } from "../api";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { ask } from "../ui/ask";
import { Icon } from "../ui/Icon";
import { msg, plural, t } from "../i18n";
import { blockingChains, type Chain, matches, seconds } from "./sessions";

export type AdminView = "sessions" | "locks";

/** The server's sessions and locks, read again every few seconds. */
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

interface Snapshot {
  sessions: ServerSession[];
  /** `null` until the Locks view asks, or when the server would not tell. */
  locks: ServerLock[] | null;
  locksError: string | null;
  at: Date;
}

export function AdminTab({ tab, connection, active, sideSession, onChange }: TabProps<AdminTabModel>) {
  const view = tab.view ?? "sessions";
  const interval = tab.interval ?? 5;
  const [caps, setCaps] = useState<Capabilities | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [onlyActive, setOnlyActive] = useState(false);
  const [onlyWaiting, setOnlyWaiting] = useState(false);
  const [version, setVersion] = useState(0);
  const busy = useRef(false);

  // Reads the lists on the navigator's session: it stays in autocommit, where PostgreSQL's
  // activity is fresh at each statement, not frozen for a transaction.
  async function load() {
    if (busy.current) return;
    busy.current = true;
    setLoading(true);
    try {
      const sid = await sideSession();
      if (sid === null) return;
      const capabilities = workbench().capabilities(sid) ?? null;
      setCaps(capabilities);
      if (!capabilities?.server_sessions) return;
      const sessions = await api.serverSessions(sid);
      let locks: ServerLock[] | null = null;
      let locksError: string | null = null;
      if (view === "locks") {
        try {
          locks = await api.serverLocks(sid);
        } catch (e) {
          locksError = toCommandError(e).message;
        }
      }
      setSnapshot({ sessions, locks, locksError, at: new Date() });
      setError(null);
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      busy.current = false;
      setLoading(false);
    }
  }

  useEffect(() => {
    if (!active) return;
    void load();
    if (interval <= 0) return;
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
  const shown = sessions.filter((s) => (!onlyActive || s.query_seconds !== null) && matches(s, filter));
  const chosen = sessions.find((s) => s.id === selected) ?? null;
  const blocked = sessions.filter((s) => s.blocked_by.length > 0).length;
  const unsupported = caps !== null && !caps.server_sessions;

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
          {INTERVALS.map(([value, label]) => (
            <option key={value} value={value}>
              {t(label)}
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
        <button className={view === "sessions" ? "active" : ""} onClick={() => onChange({ view: "sessions" })}>
          {t("Sessions")} <span className="count">{sessions.length}</span>
        </button>
        <button className={view === "locks" ? "active" : ""} onClick={() => onChange({ view: "locks" })}>
          {t("Locks")}
          {blocked > 0 && (
            <span className="count admin-blocked-count" title={t("{n} waiting for a lock", { n: blocked })}>
              {blocked}
            </span>
          )}
        </button>
      </nav>
      {unsupported && (
        <div className="notice small">{t("This database does not list the sessions of its server.")}</div>
      )}
      {error && <div className="notice error small">{error}</div>}
      {!unsupported && view === "sessions" && (
        <div className="object-body admin-body">
          <div className="admin-toolbar">
            <input
              type="search"
              placeholder={t("Filter by user, database, client or statement")}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
            <label className="check small">
              <input type="checkbox" checked={onlyActive} onChange={(e) => setOnlyActive(e.target.checked)} />
              {t("Only running statements")}
            </label>
            <span className="muted small">
              {plural(shown.length, "{n} session", "{n} sessions")}
              {blocked > 0 && ` · ${t("{n} waiting for a lock", { n: blocked })}`}
            </span>
          </div>
          <SessionTable sessions={shown} selected={selected} onSelect={setSelected} />
          {chosen && (
            <SessionDetails
              session={chosen}
              caps={caps}
              onEnd={(how) => void end(chosen, how)}
              onOpen={() => chosen.query && workbench().openTab("query", connection.id, { sql: chosen.query })}
            />
          )}
        </div>
      )}
      {!unsupported && view === "locks" && (
        <div className="object-body admin-body">
          <Locks
          sessions={sessions}
          locks={snapshot?.locks ?? null}
          locksError={snapshot?.locksError ?? null}
          onlyWaiting={onlyWaiting}
          onOnlyWaiting={setOnlyWaiting}
          onSelect={(id) => {
            setSelected(id);
            onChange({ view: "sessions" });
          }}
          />
        </div>
      )}
    </div>
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
    <div className="admin-scroll">
      <table className="meta-table admin-table">
        <thead>
          <tr>
            <th className="num">{t("Id")}</th>
            <th>{t("User")}</th>
            <th>{t("Database")}</th>
            <th>{t("Client")}</th>
            <th>{t("Application")}</th>
            <th>{t("State")}</th>
            <th className="num">{t("Running for")}</th>
            <th>{t("Waits for")}</th>
            <th>{t("Blocked by")}</th>
            <th>{t("Statement")}</th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((s) => (
            <tr
              key={s.id}
              className={[
                "admin-session",
                s.id === selected && "selected",
                s.blocked_by.length > 0 && "blocked",
                s.current && "current",
              ]
                .filter(Boolean)
                .join(" ")}
              onClick={() => onSelect(s.id)}
            >
              <td className="num">
                {s.id}
                {s.current && (
                  <span className="admin-own muted" title={t("The session of this app that reads the list")}>
                    {" "}
                    ({t("this app")})
                  </span>
                )}
              </td>
              <td>{s.user}</td>
              <td>{s.database}</td>
              <td>{s.client}</td>
              <td>{s.application}</td>
              <td>{s.state}</td>
              <td className="num">{seconds(s.query_seconds)}</td>
              <td className="admin-wait">{s.wait}</td>
              <td className="admin-wait">{s.blocked_by.join(", ")}</td>
              <td className="admin-query" title={s.query ?? undefined}>
                {s.query}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {sessions.length === 0 && <p className="muted small">{t("No sessions match.")}</p>}
    </div>
  );
}

function SessionDetails({
  session,
  caps,
  onEnd,
  onOpen,
}: {
  session: ServerSession;
  caps: Capabilities | null;
  onEnd: (how: EndSession) => void;
  onOpen: () => void;
}) {
  const running = session.query_seconds !== null;
  return (
    <section className="admin-details">
      <div className="admin-details-head">
        <strong>{t("Session {id}", { id: session.id })}</strong>
        {session.started && <span className="muted small">{t("since {time}", { time: session.started })}</span>}
        <span className="spacer" />
        <button disabled={!session.query} onClick={onOpen}>
          {t("Open in editor")}
        </button>
        {caps?.cancel_server_queries && (
          <button
            disabled={!running || session.current}
            title={running ? undefined : t("It runs no statement")}
            onClick={() => onEnd("cancel")}
          >
            {t("Cancel statement")}
          </button>
        )}
        <button
          className="danger"
          disabled={session.current}
          title={session.current ? t("This is the session of this app") : undefined}
          onClick={() => onEnd("terminate")}
        >
          {t("End session")}
        </button>
      </div>
      <pre className="admin-statement">{session.query ?? t("No statement")}</pre>
    </section>
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
    <div className="admin-scroll">
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
        <table className="meta-table admin-table">
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
              <tr key={i} className={l.granted ? "" : "blocked"}>
                <td className="num">
                  <button className="link" onClick={() => onSelect(l.session)}>
                    {l.session}
                  </button>
                </td>
                <td>{l.kind}</td>
                <td>{l.object}</td>
                <td>{l.mode}</td>
                <td className="admin-wait">{t(l.granted ? "Held" : "Waiting")}</td>
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
      <button className="admin-chain-session" onClick={() => onSelect(s.id)}>
        <span
          className={[chain.waiting.length > 0 && "admin-blocker", s.blocked_by.length > 0 && "admin-waiter"]
            .filter(Boolean)
            .join(" ")}
        >
          {s.id}
        </span>
        <span className="muted"> {[s.user, s.state, seconds(s.query_seconds)].filter(Boolean).join(" · ")}</span>
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
