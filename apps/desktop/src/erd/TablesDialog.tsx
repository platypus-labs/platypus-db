import { useState } from "react";
import { t, plural } from "../i18n";

interface Props {
  /** Every table of the schema. */
  all: string[];
  /** The tables shown now; `null` for all of them. */
  chosen: string[] | null;
  /** `null` for all the tables. */
  onSave: (chosen: string[] | null) => void;
  onClose: () => void;
}

/** Picks the tables of an ER diagram. */
export function TablesDialog({ all, chosen, onSave, onClose }: Props) {
  const [picked, setPicked] = useState(() => new Set(chosen ?? all));
  const [filter, setFilter] = useState("");
  const shown = all.filter((name) => name.toLowerCase().includes(filter.trim().toLowerCase()));
  const toggle = (names: string[], on: boolean) =>
    setPicked((p) => {
      const next = new Set(p);
      for (const n of names) {
        if (on) next.add(n);
        else next.delete(n);
      }
      return next;
    });
  const save = () => {
    onSave(picked.size === all.length ? null : all.filter((n) => picked.has(n)));
    onClose();
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal erd-tables"
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2>{t("Tables of the diagram")}</h2>
        <input autoFocus placeholder={t("Filter")} aria-label={t("Filter")} value={filter} onChange={(e) => setFilter(e.target.value)} />
        <div className="row small">
          <button type="button" className="link" onClick={() => toggle(shown, true)}>
            {t("Select shown")}
          </button>
          <button type="button" className="link" onClick={() => toggle(shown, false)}>
            {t("Clear shown")}
          </button>
          <span className="spacer" />
          <span className="muted">{plural(picked.size, "{n} table chosen", "{n} tables chosen")}</span>
        </div>
        <ul className="erd-table-list">
          {shown.map((name) => (
            <li key={name}>
              <label>
                <input type="checkbox" checked={picked.has(name)} onChange={(e) => toggle([name], e.target.checked)} />
                {name}
              </label>
            </li>
          ))}
        </ul>
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" disabled={picked.size === 0}>
            {t("Show")}
          </button>
        </div>
      </form>
    </div>
  );
}
