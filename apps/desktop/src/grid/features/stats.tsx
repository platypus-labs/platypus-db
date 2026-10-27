// Totals of the selected cells under the grid: count, and sum, average, minimum and maximum
// of the number columns. Computed by the backend from the loaded values, after the
// selection stops changing.
import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { api, type SelectionStats } from "../../api";
import type { GridApi } from "../DataGrid";
import { formatExact } from "../exactNumber";
import { formatCount, t } from "../../i18n";

/** Waits for the selection to settle, e.g. while dragging. */
const DELAY_MS = 150;

const number = new Intl.NumberFormat(undefined, { maximumFractionDigits: 6 });

export function useSelectionStats(jobId: number, grid: RefObject<GridApi | null>) {
  const [stats, setStats] = useState<SelectionStats | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const request = useRef(0);

  const onSelectionChange = useCallback(() => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      const g = grid.current;
      const sel = g?.selection();
      const token = ++request.current;
      // One cell says nothing a total would.
      if (!g || !sel || (sel.r0 === sel.r1 && sel.cols.length === 1)) return setStats(null);
      const last = g.dataRow(sel.r1);
      if (last === null) return setStats(null);
      const first = g.dataRow(sel.r0) ?? 0;
      api.selectionStats(jobId, first, last - first + 1, sel.cols).then(
        (s) => token === request.current && setStats(s),
        () => token === request.current && setStats(null),
      );
    }, DELAY_MS);
  }, [jobId, grid]);
  useEffect(() => () => clearTimeout(timer.current), []);

  const summary = stats && (
    <div className="grid-summary small">
      <span title={t("{cells} cells, {nulls} NULL", { cells: formatCount(stats.cells), nulls: formatCount(stats.cells - stats.values) })}>
        {t("Count {n}", { n: formatCount(stats.values) })}
      </span>
      {stats.sum !== null && (
        <>
          <span>{t("Sum {n}", { n: formatExact(number, stats.sum, stats.sum_exact) })}</span>
          <span>{t("Avg {n}", { n: number.format(stats.avg!) })}</span>
          <span>{t("Min {n}", { n: formatExact(number, stats.min!, stats.min_exact) })}</span>
          <span>{t("Max {n}", { n: formatExact(number, stats.max!, stats.max_exact) })}</span>
        </>
      )}
    </div>
  );
  return { onSelectionChange, summary };
}
