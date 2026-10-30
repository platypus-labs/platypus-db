import { type RefObject, useEffect, useState } from "react";
import { api, type ColumnInfo, type ColumnStats, toCommandError } from "../api";
import type { GridApi } from "../grid/DataGrid";
import { formatExact } from "../grid/exactNumber";
import { formatCount, t, plural } from "../i18n";
import { EmptyState } from "../ui/State";
import { Icon } from "../ui/Icon";

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
        <strong className="small value-name">{t("Calculate")}</strong>
        <span className="spacer" />
        <button className="icon" title={t("Close")} aria-label={t("Close")} onClick={onClose}>
          <Icon name="close" />
        </button>
      </div>
      {error && <div className="error-text small value-empty">{error}</div>}
      {!stats && !error && <EmptyState compact className="value-empty" title={t("Select cells of loaded rows.")} />}
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
          {plural(rows, "{n} row selected", "{n} rows selected")}
        </div>
      )}
    </aside>
  );
}

function lines(s: ColumnStats): [string, string][] {
  const out: [string, string][] = [
    [t("Count"), formatCount(s.values)],
    ["NULL", formatCount(s.nulls)],
    [t("Distinct"), formatCount(s.distinct)],
  ];
  const n = s.numbers;
  if (n) {
    out.push(
      [t("Sum"), formatExact(number, n.sum, n.sum_exact)],
      [t("Average"), number.format(n.avg)],
      [t("Minimum"), formatExact(number, n.min, n.min_exact)],
      [t("Maximum"), formatExact(number, n.max, n.max_exact)],
      [t("Median"), number.format(n.median)],
    );
    if (n.stddev !== null) out.push([t("Std deviation"), number.format(n.stddev)]);
  } else if (s.first !== null && s.last !== null) {
    out.push([t("First"), s.first], [t("Last"), s.last]);
  }
  return out;
}
