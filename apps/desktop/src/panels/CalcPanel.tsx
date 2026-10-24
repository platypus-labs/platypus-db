import { type RefObject, useEffect, useState } from "react";
import { api, type ColumnInfo, type ColumnStats, toCommandError } from "../api";
import type { GridApi } from "../grid/DataGrid";

interface Props {
  jobId: number;
  columns: ColumnInfo[];
  grid: RefObject<GridApi | null>;
  /** Changes with the selection, to compute again. */
  selection: number;
  onClose: () => void;
}

/** Waits for the selection to settle, e.g. while dragging. */
const DELAY_MS = 150;

const number = new Intl.NumberFormat(undefined, { maximumFractionDigits: 6 });

/** Statistics of each selected column over the selected loaded rows. */
export function CalcPanel({ jobId, columns, grid, selection, onClose }: Props) {
  const [stats, setStats] = useState<ColumnStats[] | null>(null);
  const [rows, setRows] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const timer = setTimeout(() => {
      const g = grid.current;
      const sel = g?.selection();
      // New rows not saved yet are not in the result: the loaded rows of the selection.
      const first = sel ? g!.dataRow(sel.r0) ?? 0 : 0;
      const last = sel ? g!.dataRow(sel.r1) : null;
      if (!sel || last === null) {
        setStats(null);
        return;
      }
      api.columnStats(jobId, first, last - first + 1, sel.cols).then(
        (s) => {
          if (!live) return;
          setStats(s);
          setRows(last - first + 1);
          setError(null);
        },
        (e) => live && setError(toCommandError(e).message),
      );
    }, DELAY_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [jobId, selection]);

  return (
    <aside className="value-panel calc-panel">
      <div className="value-bar">
        <strong className="small value-name">Calculate</strong>
        <span className="spacer" />
        <button className="icon" title="Close" onClick={onClose}>
          ×
        </button>
      </div>
      {error && <div className="error-text small value-empty">{error}</div>}
      {!stats && !error && <div className="muted small value-empty">Select cells of loaded rows.</div>}
      {stats && (
        <div className="calc-list">
          {stats.map((s) => (
            <section key={s.column}>
              <h3 className="small" title={columns[s.column]?.db_type}>
                {columns[s.column]?.name ?? "?"}
              </h3>
              <table className="small">
                <tbody>
                  {lines(s).map(([label, value]) => (
                    <tr key={label}>
                      <th>{label}</th>
                      <td>{value}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
          ))}
        </div>
      )}
      {stats && (
        <div className="muted small value-size">
          {rows.toLocaleString()} {rows === 1 ? "row" : "rows"} selected
        </div>
      )}
    </aside>
  );
}

function lines(s: ColumnStats): [string, string][] {
  const out: [string, string][] = [
    ["Count", s.values.toLocaleString()],
    ["NULL", s.nulls.toLocaleString()],
    ["Distinct", s.distinct.toLocaleString()],
  ];
  const n = s.numbers;
  if (n) {
    out.push(
      ["Sum", number.format(n.sum)],
      ["Average", number.format(n.avg)],
      ["Minimum", number.format(n.min)],
      ["Maximum", number.format(n.max)],
      ["Median", number.format(n.median)],
    );
    if (n.stddev !== null) out.push(["Std deviation", number.format(n.stddev)]);
  } else if (s.first !== null && s.last !== null) {
    out.push(["First", s.first], ["Last", s.last]);
  }
  return out;
}
