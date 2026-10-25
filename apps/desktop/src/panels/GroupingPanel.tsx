import { useEffect, useState } from "react";
import { api, type ColumnInfo, type GroupCounts, toCommandError } from "../api";
import { t, plural } from "../i18n";

interface Props {
  jobId: number;
  /** Result columns grouped by. */
  cols: number[];
  columns: ColumnInfo[];
  /** Loaded rows: counting again when more arrive. */
  rows: number;
  onClose: () => void;
}

/** How many loaded rows have each value (or combination) of the chosen columns. */
export function GroupingPanel({ jobId, cols, columns, rows, onClose }: Props) {
  const [counts, setCounts] = useState<GroupCounts | null>(null);
  const [error, setError] = useState<string | null>(null);
  const key = cols.join(",");

  useEffect(() => {
    let live = true;
    api.groupCounts(jobId, cols).then(
      (c) => {
        if (!live) return;
        setCounts(c);
        setError(null);
      },
      (e) => live && setError(toCommandError(e).message),
    );
    return () => {
      live = false;
    };
  }, [jobId, key, rows]);

  const names = cols.map((c) => columns[c]?.name ?? "?");
  const max = counts?.groups[0]?.count ?? 1;
  return (
    <aside className="value-panel grouping-panel">
      <div className="value-bar">
        <strong className="small value-name" title={names.join(", ")}>
          {t("Count by {columns}", { columns: names.join(", ") })}
        </strong>
        <span className="spacer" />
        <button className="icon" title={t("Close")} aria-label={t("Close")} onClick={onClose}>
          ×
        </button>
      </div>
      {error && <div className="error-text small value-empty">{error}</div>}
      {!counts && !error && <div className="muted small value-empty">{t("Counting…")}</div>}
      {counts && (
        <div className="grouping-list">
          <table>
            <thead>
              <tr>
                {names.map((n, i) => (
                  <th key={i}>{n}</th>
                ))}
                <th className="num">{t("Rows")}</th>
              </tr>
            </thead>
            <tbody>
              {counts.groups.map((g, i) => (
                <tr key={i}>
                  {g.values.map((v, j) => (
                    <td key={j} className={v === null ? "null" : undefined}>
                      {v ?? "NULL"}
                    </td>
                  ))}
                  <td className="num">
                    <span className="bar" style={{ width: `${(g.count / max) * 100}%` }} />
                    {g.count.toLocaleString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {counts && (
        <div className="muted small value-size">
          {plural(counts.distinct, "{n} value in {rows} loaded rows", "{n} values in {rows} loaded rows", { rows: counts.rows.toLocaleString() })}
          {counts.distinct > counts.groups.length && ` ${t("(top {n} shown)", { n: counts.groups.length.toLocaleString() })}`}
        </div>
      )}
    </aside>
  );
}
