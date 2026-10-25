// The health of a tab's session: a light query every few seconds while the tab is shown
// and idle, which measures the latency and reopens a connection the server dropped.
import { useEffect, useState } from "react";
import { api, toCommandError } from "../api";
import { defineSetting, useSetting } from "../settings";
import { t } from "../i18n";

export const keepAliveSeconds = defineSetting({
  key: "connections.keepalive_seconds",
  section: "Connections",
  label: "Check idle connections every (seconds)",
  description: "Measures the latency and reopens dropped connections before the next statement. 0 turns it off.",
  default: 60,
  control: { type: "number", min: 0, step: 10 },
});

export type Health =
  | { state: "ok"; latencyMs: number; reconnected: boolean }
  | { state: "lost"; message: string };

/** Pings the session that `current` gives (never opening one) after it has been `idle` for
 *  the set interval, and again at each one: never right after a statement. */
export function useSessionHealth(current: () => Promise<number | null> | null, idle: boolean): Health | null {
  const seconds = useSetting(keepAliveSeconds);
  const [health, setHealth] = useState<Health | null>(null);

  useEffect(() => {
    if (!idle || seconds <= 0) return;
    let live = true;
    const ping = async () => {
      const session = await current();
      if (session === null || !live) return;
      try {
        const h = await api.pingSession(session);
        if (live && h.state === "ok") {
          setHealth({ state: "ok", latencyMs: h.latency_ms, reconnected: h.reconnected });
        }
      } catch (e) {
        if (live) setHealth({ state: "lost", message: toCommandError(e).message });
      }
    };
    const timer = setInterval(() => void ping(), seconds * 1000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [idle, seconds]);

  return health;
}

/** A dot and the latency, for the status bar. */
export function HealthBadge({ health }: { health: Health | null }) {
  if (!health) return null;
  if (health.state === "lost") {
    return (
      <span className="health lost" title={health.message}>
        <span className="dot">●</span> {t("connection lost")}
      </span>
    );
  }
  return (
    <span
      className={`health ${health.reconnected ? "reconnected" : "ok"}`}
      title={t(health.reconnected ? "The connection was lost and has been reopened" : "Latency of the last check")}
    >
      <span className="dot">●</span> {health.latencyMs} ms{health.reconnected ? ` · ${t("reconnected")}` : ""}
    </span>
  );
}
