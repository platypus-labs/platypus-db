import { useCallback, useEffect, useRef, useState } from "react";
import { api, toCommandError, type HistoryEntry } from "../api";
import { fmt, formatMs } from "./ResultPanel";

const PAGE = 100;

interface Props {
  connectionId: string;
  /** Bumped after each run so the list picks up new entries. */
  refresh: number;
  onInsert: (sql: string) => void;
  onClose: () => void;
}

function when(ms: number) {
  const d = new Date(ms);
  const today = new Date().toDateString() === d.toDateString();
  return today
    ? d.toLocaleTimeString()
    : `${d.toLocaleDateString()} ${d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
}

export function HistoryPanel({ connectionId, refresh, onInsert, onClose }: Props) {
  const [text, setText] = useState("");
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [more, setMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const search = useRef(0);

  const load = useCallback(
    async (beforeId?: number) => {
      const token = ++search.current;
      try {
        const page = await api.searchHistory({ connectionIds: [connectionId], text, beforeId, limit: PAGE });
        if (token !== search.current) return;
        setEntries((prev) => (beforeId === undefined ? page : [...prev, ...page]));
        setMore(page.length === PAGE);
        setError(null);
      } catch (e) {
        setError(toCommandError(e).message);
      }
    },
    [connectionId, text],
  );

  useEffect(() => {
    const t = setTimeout(() => void load(), 150);
    return () => clearTimeout(t);
  }, [load, refresh]);

  async function remove(id: number) {
    await api.deleteHistory(id);
    setEntries((e) => e.filter((x) => x.id !== id));
  }

  return (
    <aside className="history">
      <div className="history-header">
        <strong>History</strong>
        <span className="spacer" />
        <button className="icon" title="Close" onClick={onClose}>
          ×
        </button>
      </div>
      <input
        className="history-search"
        placeholder="Search SQL…"
        value={text}
        onChange={(e) => setText(e.target.value)}
      />
      {error && <div className="notice error small">{error}</div>}
      <ul className="history-list">
        {entries.map((e) => (
          <li
            key={e.id}
            className={`history-entry ${e.status}`}
            title={e.error ?? "Double-click to insert into the editor"}
            onDoubleClick={() => onInsert(e.sql)}
          >
            <pre>{e.sql}</pre>
            <div className="history-meta">
              <span>{when(e.started_at)}</span>
              <span>{formatMs(e.duration_ms)}</span>
              {e.rows !== null && <span>{fmt.format(e.rows)} rows</span>}
              {e.status !== "ok" && <span className="history-status">{e.status}</span>}
              <span className="spacer" />
              <button className="icon" title="Insert into editor" onClick={() => onInsert(e.sql)}>
                ↵
              </button>
              <button className="icon" title="Delete" onClick={() => void remove(e.id)}>
                ×
              </button>
            </div>
          </li>
        ))}
        {entries.length === 0 && !error && <li className="muted history-empty">No history yet</li>}
      </ul>
      {more && (
        <button className="link history-more" onClick={() => void load(entries.at(-1)?.id)}>
          Load more
        </button>
      )}
    </aside>
  );
}
