import { useState } from "react";
import type { SavedConnection } from "../api";
import { navigatorFilter, setNavigatorFilter } from "./prefs";

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
        <h2>Navigator filters: {connection.name}</h2>
        <p className="muted small">
          Names separated by commas; <code>*</code> matches any text, case is ignored.
        </p>
        <label className="field">
          <span>Hide schemas</span>
          <input autoFocus placeholder="tmp_*, audit" value={schemas} onChange={(e) => setSchemas(e.target.value)} />
        </label>
        <label className="field">
          <span>Hide objects</span>
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
            Remove filters
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="primary">
            Save
          </button>
        </div>
      </form>
    </div>
  );
}
