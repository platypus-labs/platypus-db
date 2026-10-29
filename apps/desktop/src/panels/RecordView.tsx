import { useEffect, useRef, useState } from "react";
import { api, type CellInput, type ColumnInfo, toCommandError } from "../api";
import { shown as masked, useMask } from "../guards/mask";
import { formatCount, t } from "../i18n";

interface Props {
  jobId: number;
  columns: ColumnInfo[];
  /** Result columns to show, in the grid's order. */
  shown: number[];
  /** Grid row shown, and the number of grid rows. */
  row: number;
  rowCount: number;
  /** Grid rows before the result's: new rows of the editor. */
  topRows: number;
  /** Bumped when rows load. */
  version: number;
  /** The data editor's change of a cell not saved yet, by grid row. */
  pending?: (row: number, col: number) => CellInput | undefined;
  onMove: (row: number) => void;
  onClose: () => void;
}

/** A pending value as the record shows it. */
function pendingText(input: CellInput): string | null {
  return input.kind === "value" ? input.text : input.kind === "null" ? null : "DEFAULT";
}

/** One row of the grid, top to bottom, with the full values; moves through the rows. */
export function RecordView({ jobId, columns, shown, row, rowCount, topRows, version, pending, onMove, onClose }: Props) {
  const [loaded, setLoaded] = useState<{ row: number; values: (string | null)[] | null } | null>(null);
  const mask = useMask();
  const [error, setError] = useState<string | null>(null);
  const dataRow = row - topRows;
  // Another row's values never show under this row's number while it loads.
  const values = loaded?.row === dataRow ? loaded.values : null;
  const box = useRef<HTMLDivElement>(null);
  // The arrows move through the rows from the start.
  useEffect(() => box.current?.focus(), []);

  useEffect(() => {
    if (dataRow < 0) return setLoaded(null);
    let live = true;
    api.rowValues(jobId, dataRow, 1).then(
      ([r]) => {
        if (!live) return;
        setLoaded({ row: dataRow, values: r ?? null });
        setError(null);
      },
      (e) => live && setError(toCommandError(e).message),
    );
    return () => {
      live = false;
    };
  }, [jobId, dataRow, version]);

  function onKeyDown(e: React.KeyboardEvent) {
    const moves: Record<string, number> = { ArrowLeft: row - 1, ArrowRight: row + 1, Home: 0, End: rowCount - 1 };
    const next = moves[e.key];
    if (next !== undefined) {
      e.preventDefault();
      onMove(Math.max(0, Math.min(rowCount - 1, next)));
    } else if (e.key === "Escape") onClose();
  }

  return (
    <div className="record-view" ref={box} tabIndex={0} onKeyDown={onKeyDown}>
      <div className="value-bar">
        <button onClick={() => onMove(0)} disabled={row <= 0} title={t("First row (Home)")} aria-label={t("First row (Home)")}>
          ⏮
        </button>
        <button onClick={() => onMove(row - 1)} disabled={row <= 0} title={t("Previous row (←)")} aria-label={t("Previous row (←)")}>
          ◀
        </button>
        <span className="small record-position">
          {dataRow < 0
            ? t("New row")
            : t("Row {n} of {count}", { n: formatCount(dataRow + 1), count: formatCount(rowCount - topRows) })}
        </span>
        <button onClick={() => onMove(row + 1)} disabled={row >= rowCount - 1} title={t("Next row (→)")} aria-label={t("Next row (→)")}>
          ▶
        </button>
        <button onClick={() => onMove(rowCount - 1)} disabled={row >= rowCount - 1} title={t("Last row (End)")} aria-label={t("Last row (End)")}>
          ⏭
        </button>
        <span className="spacer" />
        <button className="icon" title={t("Back to the grid")} aria-label={t("Back to the grid")} onClick={onClose}>
          ×
        </button>
      </div>
      {error && <div className="error-text small value-empty">{error}</div>}
      {dataRow < 0 ? (
        <div className="muted small value-empty">{t("A new row of the editor: it shows in the grid until saved.")}</div>
      ) : (
        <div className="record-fields">
          {shown.map((c) => {
            const column = columns[c]!;
            // An edit not saved yet shows in place of the loaded value, as in the grid.
            const change = pending?.(row, c);
            const value = masked(change ? pendingText(change) : values?.[c], !!mask?.(column));
            return (
              <div key={c} className="record-field">
                <div className="record-name" title={column.db_type}>
                  {column.name}
                  <span className="muted small"> {column.db_type}</span>
                </div>
                <div className={`record-value${value === null ? " null" : ""}`}>
                  {values || change ? (value ?? "NULL") : "…"}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
