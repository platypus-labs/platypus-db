import { type ReactNode, type RefObject, useEffect, useMemo, useRef, useState } from "react";
import { api, toCommandError } from "../../api";
import type { CellDecoration, GridApi } from "../DataGrid";
import { t } from "../../i18n";

const MATCH: CellDecoration = { background: "rgba(255, 212, 59, 0.45)" };
const CURRENT: CellDecoration = { background: "rgba(255, 146, 43, 0.75)" };
const DEBOUNCE_MS = 150;

export interface GridSearch {
  /** Opens the search box, or focuses it when open. */
  open(): void;
  /** The search box, placed over the grid's top right corner. */
  box: ReactNode;
  /** Highlights matches; `undefined` while there are none. */
  decorate?: (row: number, col: number) => CellDecoration | undefined;
}

interface Found {
  matches: [number, number][];
  /** `row * columns + col` of every match, for the per-cell highlight. */
  keys: Set<number>;
  truncated: boolean;
}

/**
 * Ctrl+F in a result: finds the loaded cells containing the text (ignoring case), highlights
 * them and steps through them. `topRows` are grid rows above the result's, e.g. new rows.
 */
export function useGridSearch(
  jobId: number | null,
  columns: number,
  topRows: number,
  grid: RefObject<GridApi | null>,
): GridSearch {
  const [visible, setVisible] = useState(false);
  const [text, setText] = useState("");
  const [found, setFound] = useState<Found | null>(null);
  const [index, setIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!visible || jobId === null || !text) {
      setFound(null);
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      api.searchRows(jobId, text).then(
        (r) => {
          if (!live) return;
          const matches = r.matches;
          setFound({ matches, keys: new Set(matches.map(([row, col]) => row * columns + col)), truncated: r.truncated });
          setIndex(0);
          setError(null);
          if (matches[0]) grid.current?.select({ row: matches[0][0] + topRows, col: matches[0][1] });
        },
        (e) => live && setError(toCommandError(e).message),
      );
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
    // Re-runs for a new text or result, not when rows are added on top.
  }, [visible, jobId, text, columns]);

  function step(delta: number) {
    const n = found?.matches.length ?? 0;
    if (!found || n === 0) return;
    const next = (index + delta + n) % n;
    setIndex(next);
    const [row, col] = found.matches[next]!;
    grid.current?.select({ row: row + topRows, col });
  }

  function close() {
    setVisible(false);
    setText("");
    grid.current?.focus();
  }

  // Stable between renders, so the grid redraws only when the matches change.
  const decorate = useMemo(() => {
    if (!found || found.keys.size === 0) return undefined;
    const current = found.matches[index];
    return (row: number, col: number) => {
      const r = row - topRows;
      if (r < 0 || !found.keys.has(r * columns + col)) return undefined;
      return current && current[0] === r && current[1] === col ? CURRENT : MATCH;
    };
  }, [found, index, topRows, columns]);

  const count = found?.matches.length ?? 0;
  const box = visible && (
    <div className="grid-search" onMouseDown={(e) => e.stopPropagation()}>
      <input
        ref={input}
        autoFocus
        value={text}
        placeholder={t("Find in loaded rows")}
        aria-label={t("Find in loaded rows")}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Escape") close();
          else if (e.key === "Enter") step(e.shiftKey ? -1 : 1);
        }}
      />
      <span className={`small ${error ? "error-text" : "muted"}`} title={error ?? undefined}>
        {error ? t("error") : !text ? "" : count === 0 ? t("no matches") : t("{index} of {count}", { index: index + 1, count: `${count}${found?.truncated ? "+" : ""}` })}
      </span>
      <button className="icon" onClick={() => step(-1)} disabled={count === 0} title={t("Previous (Shift+Enter)")} aria-label={t("Previous (Shift+Enter)")}>
        ↑
      </button>
      <button className="icon" onClick={() => step(1)} disabled={count === 0} title={t("Next (Enter)")} aria-label={t("Next (Enter)")}>
        ↓
      </button>
      <button className="icon" onClick={close} title={t("Close (Escape)")} aria-label={t("Close (Escape)")}>
        ×
      </button>
    </div>
  );

  return {
    open: () => {
      setVisible(true);
      input.current?.focus();
      input.current?.select();
    },
    box,
    decorate,
  };
}
