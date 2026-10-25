import { useState } from "react";
import type { SavedConnection } from "../api";
import { navigatorFilter, setNavigatorFilter } from "./prefs";
import { t, tNodes } from "../i18n";

/** Edits the name patterns of the schemas and objects the navigator hides for a connection. */
export function FilterDialog({ connection, onClose }: { connection: SavedConnection; onClose: () => void }) {
  const saved = navigatorFilter(connection.id);
  const [schemas, setSchemas] = useState(saved?.schemas ?? "");
  const [objects, setObjects] = useState(saved?.objects ?? "");

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <form
        className="modal navigator-filter"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          setNavigatorFilter(connection.id, { schemas, objects });
          onClose();
        }}
      >
        <h2>{t("Navigator filters: {name}", { name: connection.name })}</h2>
        <p className="muted small">{tNodes("Names separated by commas; {star} matches any text, case is ignored.", { star: <code>*</code> })}</p>
        <label className="field">
          <span>{t("Hide schemas")}</span>
          <input autoFocus placeholder="tmp_*, audit" value={schemas} onChange={(e) => setSchemas(e.target.value)} />
        </label>
        <label className="field">
          <span>{t("Hide objects")}</span>
          <input placeholder="*_backup, old_*" value={objects} onChange={(e) => setObjects(e.target.value)} />
        </label>
        <div className="actions">
          <button
            type="button"
            className="link"
            disabled={!saved}
            onClick={() => {
              setNavigatorFilter(connection.id, null);
              onClose();
            }}
          >
            {t("Remove filters")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary">
            {t("Save")}
          </button>
        </div>
      </form>
    </div>
  );
}
