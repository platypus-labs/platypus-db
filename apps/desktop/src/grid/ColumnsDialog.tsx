import type { ColumnInfo } from "../api";
import { type ColumnLayout, hideColumn, moveColumn, pinColumn, resolveLayout } from "./layout";
import { t } from "../i18n";

interface Props {
  columns: ColumnInfo[];
  layout: ColumnLayout;
  onChange: (layout: ColumnLayout) => void;
  onClose: () => void;
}

/** Shows, hides, pins and orders a grid's columns; changes apply as they are made. */
export function ColumnsDialog({ columns, layout, onChange, onClose }: Props) {
  const resolved = resolveLayout(columns, layout);
  const hidden = columns.map((_, i) => i).filter((i) => resolved.position[i] === -1);
  const show = (name: string) => onChange({ ...layout, hidden: layout.hidden.filter((n) => n !== name) });

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal small" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Columns")}</h2>
        <p className="muted small">{t("Pinned columns stay on the left while scrolling. Headers also drag to move columns.")}</p>
        <div className="columns-list">
          {resolved.view.map((c, d) => {
            const name = columns[c]!.name;
            const pinned = d < resolved.pinned;
            return (
              <div key={c} className="columns-row">
                <label className="check">
                  <input
                    type="checkbox"
                    checked
                    disabled={resolved.view.length === 1}
                    onChange={() => onChange(hideColumn(layout, name))}
                  />
                  {name}
                </label>
                <span className="spacer" />
                <button
                  className={pinned ? "toggled" : ""}
                  onClick={() => onChange(pinColumn(layout, name, !pinned))}
                  title={t(pinned ? "Unpin" : "Pin to the left")}
                >
                  {t("Pin")}
                </button>
                <button
                  onClick={() => onChange(moveColumn(layout, columns, resolved, d, d - 1))}
                  disabled={d === 0}
                  title={t("Move left")}
                  aria-label={t("Move left")}
                >
                  ↑
                </button>
                <button
                  onClick={() => onChange(moveColumn(layout, columns, resolved, d, d + 1))}
                  disabled={d === resolved.view.length - 1}
                  title={t("Move right")}
                  aria-label={t("Move right")}
                >
                  ↓
                </button>
              </div>
            );
          })}
          {hidden.map((c) => (
            <div key={c} className="columns-row muted">
              <label className="check">
                <input type="checkbox" checked={false} onChange={() => show(columns[c]!.name)} />
                {columns[c]!.name}
              </label>
            </div>
          ))}
        </div>
        <div className="actions">
          <span className="spacer" />
          <button type="button" className="primary" onClick={onClose} autoFocus>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}
