import { useCallback, useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, toCommandError, type AgentCall, type AgentCallQuery, type AgentSession, type HistoryStatus } from "../api";
import { ask } from "../ui/ask";
import { t } from "../i18n";
import { EmptyState } from "../ui/State";

/** Calls a page loads. */
const PAGE = 100;

function when(ms: number): string {
  const d = new Date(ms);
  return new Date().toDateString() === d.toDateString() ? d.toLocaleTimeString() : d.toLocaleString();
}

function statusLabel(status: HistoryStatus): string {
  return status === "ok" ? t("OK") : status === "cancelled" ? t("Cancelled") : t("Error");
}

/** A call's arguments, compact: the connection is in its own column. */
function argumentsText(call: AgentCall): string {
  try {
    const args = JSON.parse(call.arguments) as Record<string, unknown>;
    delete args.connection;
    const parts = Object.entries(args).map(([k, v]) => `${k}=${typeof v === "string" ? v : JSON.stringify(v)}`);
    return parts.join("  ");
  } catch {
    return call.arguments;
  }
}

/** The Activity page of Agents: every call agents made to the tools, by session, with a
 *  filter and a CSV export. */
export function AgentActivity({ onSetup }: { onSetup: () => void }) {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [session, setSession] = useState<string>("");
  const [status, setStatus] = useState<HistoryStatus | "">("");
  const [text, setText] = useState("");
  const [calls, setCalls] = useState<AgentCall[]>([]);
  const [more, setMore] = useState(false);
  const [open, setOpen] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const query = useCallback(
    (before_id?: number): AgentCallQuery => ({
      text: text.trim() || null,
      session: session || null,
      status: status || null,
      before_id: before_id ?? null,
      limit: PAGE,
    }),
    [text, session, status],
  );

  const load = useCallback(() => {
    const failed = (e: unknown) => setError(toCommandError(e).message);
    api.agentSessions(50).then(setSessions, failed);
    api.agentCalls(query()).then((page) => {
      setError(null);
      setCalls(page);
      setMore(page.length === PAGE);
    }, failed);
  }, [query]);

  useEffect(() => {
    const timer = setTimeout(load, 200);
    return () => clearTimeout(timer);
  }, [load]);

  const loadMore = () => {
    const last = calls[calls.length - 1];
    if (!last) return;
    api.agentCalls(query(last.id)).then(
      (page) => {
        setCalls((c) => [...c, ...page]);
        setMore(page.length === PAGE);
      },
      (e: unknown) => setError(toCommandError(e).message),
    );
  };

  const exportCsv = async () => {
    try {
      const path = await save({ filters: [{ name: "CSV", extensions: ["csv"] }], defaultPath: "agent-calls.csv" });
      if (!path) return;
      await api.writeTextFile(path, await api.agentCallsExport(query()));
      setNotice(t("Saved {path}.", { path }));
    } catch (e) {
      setError(toCommandError(e).message);
    }
  };

  const clear = async () => {
    if (!(await ask(t("Delete every call in the agents' log?"), { confirm: t("Delete"), danger: true }))) return;
    try {
      await api.clearAgentCalls();
      setSession("");
      load();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  };

  return (
    <div className="agent-activity">
      <p className="muted small">
        {t(
          "Every call agents made to PlatypusDB's tools, newest first: each start of the MCP server is a session. The statements they ran are also in the history.",
        )}
      </p>
      <div className="row tight agent-activity-filters">
        <select value={session} onChange={(e) => setSession(e.target.value)} data-filter="session" aria-label={t("Session")}>
          <option value="">{t("All sessions")}</option>
          {sessions.map((s) => (
            <option key={s.session} value={s.session}>
              {`${when(s.first_at)} · ${s.agent} · ${t("{n} calls", { n: s.calls })}${s.errors ? ` · ${t("{n} errors", { n: s.errors })}` : ""}`}
            </option>
          ))}
        </select>
        <select
          value={status}
          onChange={(e) => setStatus(e.target.value as HistoryStatus | "")}
          data-filter="status"
          aria-label={t("Outcome")}
        >
          <option value="">{t("Any outcome")}</option>
          <option value="ok">{t("OK")}</option>
          <option value="error">{t("Error")}</option>
          <option value="cancelled">{t("Cancelled")}</option>
        </select>
        <input
          type="search"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={t("Search calls…")}
          data-filter="text"
        />
        <span className="spacer" />
        <button type="button" onClick={() => void exportCsv()} data-action="export-calls" disabled={calls.length === 0}>
          {t("Export CSV")}
        </button>
        <button type="button" onClick={() => void clear()} data-action="clear-calls" disabled={calls.length === 0}>
          {t("Clear")}
        </button>
      </div>
      {error && <div className="notice error small">{error}</div>}
      {notice && <div className="notice small" data-notice="exported">{notice}</div>}
      <table className="agents-connections agent-calls">
        <thead>
          <tr>
            <th>{t("When")}</th>
            <th>{t("Agent")}</th>
            <th>{t("Tool")}</th>
            <th>{t("Connection")}</th>
            <th>{t("Arguments")}</th>
            <th>{t("Duration")}</th>
            <th>{t("Outcome")}</th>
          </tr>
        </thead>
        <tbody>
          {calls.map((c) => (
            <tr
              key={c.id}
              data-call={c.tool}
              data-status={c.status}
              className={open === c.id ? "open" : undefined}
              onClick={() => setOpen(open === c.id ? null : c.id)}
              title={c.error ?? undefined}
            >
              <td>{when(c.started_at)}</td>
              <td>{c.agent}</td>
              <td>
                <code>{c.tool}</code>
              </td>
              <td>{c.connection ?? ""}</td>
              <td className="agent-call-args">{open === c.id ? c.arguments : argumentsText(c)}</td>
              <td>{`${c.duration_ms} ms`}</td>
              <td className={c.status === "ok" ? undefined : "agent-call-failed"}>
                {statusLabel(c.status)}
                {open === c.id && c.error && <div className="small">{c.error}</div>}
              </td>
            </tr>
          ))}
          {calls.length === 0 && (
            <tr>
              <td colSpan={7}>
                <EmptyState
                  compact
                  title={t("No agent calls yet.")}
                  actions={
                    <button type="button" onClick={onSetup}>
                      {t("Connect an agent")}
                    </button>
                  }
                />
              </td>
            </tr>
          )}
        </tbody>
      </table>
      {more && (
        <button type="button" onClick={loadMore} data-action="more-calls">
          {t("Load more")}
        </button>
      )}
    </div>
  );
}
