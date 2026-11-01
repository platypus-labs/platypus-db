import { useEffect, useRef, useState } from "react";
import { api, type LogEntry, toCommandError } from "../api";
import { fmt, formatMs } from "../components/ResultPanel";
import { useSetting } from "../settings";
import { isSlow, slowQuerySeconds } from "./slow";
import { t, msg } from "../i18n";
import { copyText } from "../ui/copy";

/** How often the open dialog asks for what changed. */
const POLL_MS = 1000;
/** As many as the app keeps; older ones are gone from it already. */
const KEPT = 2000;

const STATUS_LABELS: Record<LogEntry["status"], string> = {
  running: msg("Running"),
  fetching: msg("Reading rows"),
  done: msg("Done"),
  closed: msg("Closed early"),
  failed: msg("Failed"),
};

/** The app's own requests as the backend names them (`querylog.rs`), with the name in them. */
const REQUESTS: [RegExp, string][] = [
  [/^schemas$/, msg("schemas")],
  [/^databases$/, msg("databases")],
  [/^catalog columns$/, msg("catalog columns")],
  [/^catalog foreign keys$/, msg("catalog foreign keys")],
  [/^autocommit on$/, msg("autocommit on")],
  [/^autocommit off$/, msg("autocommit off")],
  [/^objects of (.*)$/s, msg("objects of {name}")],
  [/^table details of (.*)$/s, msg("table details of {name}")],
  [/^foreign keys to (.*)$/s, msg("foreign keys to {name}")],
  [/^DDL of (.*)$/s, msg("DDL of {name}")],
  [/^properties of (.*)$/s, msg("properties of {name}")],
  [/^search for (.*)$/s, msg("search for {name}")],
];

/** What the entry shows: a statement as sent, a request of the app in the interface's language. */
function entryText(e: LogEntry): string {
  if (e.kind === "query") return e.text;
  for (const [pattern, text] of REQUESTS) {
    const m = pattern.exec(e.text);
    if (m) return t(text, { name: m[1] ?? "" });
  }
  return e.text;
}

/** Below 10 ms the tenths tell fast from instant. */
const duration = (ms: number) => (ms < 10 ? `${ms.toFixed(1)} ms` : formatMs(Math.round(ms)));

const time = new Intl.DateTimeFormat(undefined, {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  fractionalSecondDigits: 3,
});

/** Where a statement's time went: the network (from the last keep-alive), the server
 *  until its first answer, then reading the rows. */
function trace(e: LogEntry) {
  if (e.first_ms === null) return null;
  const network = e.network_ms === null ? null : Math.min(e.network_ms, e.first_ms);
  const fetch = Math.max(0, (e.elapsed_ms ?? e.first_ms) - e.first_ms);
  return { network, server: e.first_ms - (network ?? 0), fetch };
}

function Trace({ entry }: { entry: LogEntry }) {
  const times = trace(entry);
  if (!times) return null;
  /** The part's class, its label, its time and what it is. */
  const parts: [string, string, number | null, string][] = [
    ["network", t("Network"), times.network, t("The connection's last keep-alive round trip")],
    ["server", t("Server"), times.server, t("Until the first answer, less the network")],
    ["fetch", t("Fetch"), times.fetch, t("Reading the rows after the first answer")],
  ];
  const total = parts.reduce((sum, [, , ms]) => sum + (ms ?? 0), 0);
  return (
    <div className="query-trace">
      <div className="query-trace-bar">
        {parts.map(([part, , ms]) => (
          <span key={part} className={part} style={{ flexGrow: total > 0 ? (ms ?? 0) / total : 0 }} />
        ))}
      </div>
      {parts.map(([part, label, ms, title]) => (
        <span key={part} className={part} title={title}>
          {label} {ms === null ? t("unknown") : duration(ms)}
        </span>
      ))}
    </div>
  );
}

/** What the app sent to the databases, newest first, updated while open. */
export function QueryManager({ onClose }: { onClose: () => void }) {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState("");
  const [metadata, setMetadata] = useState(true);
  const [slowOnly, setSlowOnly] = useState(false);
  const slowSeconds = useSetting(slowQuerySeconds);
  const [selected, setSelected] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const since = useRef(0);
  /** Bumped by Clear: a poll that started before it brings entries that are gone. */
  const clears = useRef(0);

  useEffect(() => {
    let live = true;
    const poll = async () => {
      const epoch = clears.current;
      try {
        const page = await api.queryLog(since.current);
        if (!live || epoch !== clears.current) return;
        since.current = page.rev;
        if (page.entries.length) {
          setEntries((old) => {
            const byId = new Map(old.map((e) => [e.id, e]));
            for (const e of page.entries) byId.set(e.id, e);
            return [...byId.values()].sort((a, b) => b.id - a.id).slice(0, KEPT);
          });
        }
      } catch (e) {
        if (live) setError(toCommandError(e).message);
      }
    };
    void poll();
    const timer = setInterval(() => void poll(), POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, []);

  async function clear() {
    clears.current++;
    try {
      await api.clearQueryLog();
    } catch (e) {
      setError(toCommandError(e).message);
      return;
    }
    // Polls started while clearing are dropped too.
    clears.current++;
    setEntries([]);
    setSelected(null);
  }

  const needle = filter.trim().toLowerCase();
  const shown = entries.filter(
    (e) =>
      (metadata || e.kind !== "metadata") &&
      (!slowOnly || isSlow(e.elapsed_ms, slowSeconds)) &&
      (!needle || entryText(e).toLowerCase().includes(needle) || e.connection.toLowerCase().includes(needle)),
  );
  const current = entries.find((e) => e.id === selected);

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="modal query-manager"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2>{t("Query manager")}</h2>
        <div className="query-manager-bar">
          <input
            autoFocus
            placeholder={t("Filter by statement or connection")}
            aria-label={t("Filter by statement or connection")}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <label>
            <input type="checkbox" checked={metadata} onChange={(e) => setMetadata(e.target.checked)} /> {t("Metadata")}
          </label>
          {slowSeconds > 0 && (
            <label title={t("Slower than {n} s (Settings)", { n: slowSeconds })}>
              <input type="checkbox" checked={slowOnly} onChange={(e) => setSlowOnly(e.target.checked)} /> {t("Slow only")}
            </label>
          )}
          <span className="spacer" />
          <span className="muted small">{t("{index} of {count}", { index: fmt.format(shown.length), count: fmt.format(entries.length) })}</span>
          <button onClick={() => void clear()}>{t("Clear")}</button>
        </div>
        {error && <div className="notice error small">{error}</div>}
        <div className="query-manager-list">
          <table className="meta-table">
            <thead>
              <tr>
                <th>{t("Time")}</th>
                <th>{t("Connection")}</th>
                <th>{t("Statement")}</th>
                <th>{t("Duration")}</th>
                <th>{t("Rows")}</th>
                <th>{t("Result")}</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((e) => (
                <tr
                  key={e.id}
                  className={`${e.id === selected ? "selected " : ""}${e.kind}${e.status === "failed" ? " failed" : ""}${isSlow(e.elapsed_ms, slowSeconds) ? " slow" : ""}`}
                  onClick={() => setSelected(e.id)}
                >
                  <td>{time.format(e.started_at)}</td>
                  <td>{e.connection}</td>
                  <td className="query-manager-text" title={entryText(e)}>
                    {entryText(e)}
                  </td>
                  <td>{e.elapsed_ms === null ? "" : duration(e.elapsed_ms)}</td>
                  <td>{e.rows === null ? "" : fmt.format(e.rows)}</td>
                  <td title={e.error ?? undefined}>{t(STATUS_LABELS[e.status])}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {current && (
          <>
            {current.error && <div className="notice error small">{current.error}</div>}
            <Trace entry={current} />
            <pre className="sql-preview">{entryText(current)}</pre>
          </>
        )}
        <div className="actions">
          {current && (
            <button type="button" onClick={() => void copyText(current.text)}>
              {t("Copy statement")}
            </button>
          )}
          <span className="spacer" />
          <button type="button" className="primary" onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}
