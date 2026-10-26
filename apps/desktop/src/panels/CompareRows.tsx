import { useEffect, useState } from "react";
import { api, type ColumnInfo, toCommandError } from "../api";
import { t } from "../i18n";

interface Props {
  jobId: number;
  columns: ColumnInfo[];
  /** The two loaded rows to compare, as result row numbers. */
  rows: [number, number];
  onClose: () => void;
}

/** Two rows side by side, column by column, with the columns that differ marked. */
export function CompareRows({ jobId, columns, rows, onClose }: Props) {
  const [values, setValues] = useState<[(string | null)[], (string | null)[]] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [onlyDifferent, setOnlyDifferent] = useState(false);
  const [a, b] = rows;

  useEffect(() => {
    Promise.all([api.rowValues(jobId, a, 1), api.rowValues(jobId, b, 1)]).then(
      ([x, y]) => setValues([x[0] ?? [], y[0] ?? []]),
      (e) => setError(toCommandError(e).message),
    );
  }, [jobId, a, b]);

  const shown = values
    ? columns.map((c, i) => ({ name: c.name, left: values[0][i] ?? null, right: values[1][i] ?? null }))
    : [];
  const differing = shown.filter((r) => r.left !== r.right).length;

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal compare-rows" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Rows {a} and {b}", { a: a + 1, b: b + 1 })}</h2>
        {error && <div className="notice error small">{error}</div>}
        {values && (
          <>
            <label className="check small">
              <input type="checkbox" checked={onlyDifferent} onChange={(e) => setOnlyDifferent(e.target.checked)} />
              {differing === 1 ? t("Only the column that differs") : t("Only the {n} columns that differ", { n: differing })}
            </label>
            <div className="compare-list">
              <table>
                <thead>
                  <tr>
                    <th>{t("Column")}</th>
                    <th>{t("Row {n}", { n: a + 1 })}</th>
                    <th>{t("Row {n}", { n: b + 1 })}</th>
                  </tr>
                </thead>
                <tbody>
                  {shown
                    .filter((r) => !onlyDifferent || r.left !== r.right)
                    .map((r, i) => (
                      <tr key={i} className={r.left !== r.right ? "differs" : undefined}>
                        <th>{r.name}</th>
                        <td className={r.left === null ? "null" : undefined}>{r.left ?? "NULL"}</td>
                        <td className={r.right === null ? "null" : undefined}>{r.right ?? "NULL"}</td>
                      </tr>
                    ))}
                </tbody>
              </table>
            </div>
          </>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" className="primary" autoFocus onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}
