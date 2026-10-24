import { useEffect, useRef, useState } from "react";
import { api, type LogEntry, toCommandError } from "../api";
import { fmt, formatMs } from "../components/ResultPanel";
import { useSetting } from "../settings";
import { isSlow, slowQuerySeconds } from "./slow";

/** How often the open dialog asks for what changed. */
const POLL_MS = 1000;
/** As many as the app keeps; older ones are gone from it already. */
const KEPT = 2000;

const STATUS_LABELS: Record<LogEntry["status"], string> = {
  running: "Running",
  fetching: "Reading rows",
  done: "Done",
  closed: "Closed early",
  failed: "Failed",
};

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
  const t = trace(entry);
  if (!t) return null;
  const parts: [string, number | null, string][] = [
    ["Network", t.network, "The connection's last keep-alive round trip"],
    ["Server", t.server, "Until the first answer, less the network"],
    ["Fetch", t.fetch, "Reading the rows after the first answer"],
  ];
  const total = parts.reduce((sum, [, ms]) => sum + (ms ?? 0), 0);
  return (
    <div className="query-trace">
      <div className="query-trace-bar">
        {parts.map(([label, ms]) => (
          <span key={label} className={label.toLowerCase()} style={{ flexGrow: total > 0 ? (ms ?? 0) / total : 0 }} />
        ))}
      </div>
      {parts.map(([label, ms, title]) => (
        <span key={label} className={label.toLowerCase()} title={title}>
          {label} {ms === null ? "unknown" : duration(ms)}
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

  useEffect(() => {
    let live = true;
    const poll = async () => {
      try {
        const page = await api.queryLog(since.current);
        if (!live) return;
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
    await api.clearQueryLog();
    setEntries([]);
    setSelected(null);
  }

  const needle = filter.trim().toLowerCase();
  const shown = entries.filter(
    (e) =>
      (metadata || e.kind !== "metadata") &&
      (!slowOnly || isSlow(e.elapsed_ms, slowSeconds)) &&
      (!needle || e.text.toLowerCase().includes(needle) || e.connection.toLowerCase().includes(needle)),
  );
  const current = entries.find((e) => e.id === selected);

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="modal query-manager"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2>Query manager</h2>
        <div className="query-manager-bar">
          <input
            autoFocus
            placeholder="Filter by statement or connection"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <label>
            <input type="checkbox" checked={metadata} onChange={(e) => setMetadata(e.target.checked)} /> Metadata
          </label>
          {slowSeconds > 0 && (
            <label title={`Slower than ${slowSeconds} s (Settings)`}>
              <input type="checkbox" checked={slowOnly} onChange={(e) => setSlowOnly(e.target.checked)} /> Slow only
            </label>
          )}
          <span className="spacer" />
          <span className="muted small">
            {fmt.format(shown.length)} of {fmt.format(entries.length)}
          </span>
          <button onClick={() => void clear()}>Clear</button>
        </div>
        {error && <div className="notice error small">{error}</div>}
        <div className="query-manager-list">
          <table className="meta-table">
            <thead>
              <tr>
                <th>Time</th>
                <th>Connection</th>
                <th>Statement</th>
                <th>Duration</th>
                <th>Rows</th>
                <th>Result</th>
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
                  <td className="query-manager-text" title={e.text}>
                    {e.text}
                  </td>
                  <td>{e.elapsed_ms === null ? "" : duration(e.elapsed_ms)}</td>
                  <td>{e.rows === null ? "" : fmt.format(e.rows)}</td>
                  <td title={e.error ?? undefined}>{STATUS_LABELS[e.status]}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {current && (
          <>
            {current.error && <div className="notice error small">{current.error}</div>}
            <Trace entry={current} />
            <pre className="sql-preview">{current.text}</pre>
          </>
        )}
        <div className="actions">
          {current && <button onClick={() => void navigator.clipboard.writeText(current.text)}>Copy statement</button>}
          <button onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}
