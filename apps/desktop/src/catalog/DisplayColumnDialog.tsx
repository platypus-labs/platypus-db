import { useEffect, useState } from "react";
import { api, toCommandError } from "../api";
import { displayColumn, setDisplayColumn, useCatalogVersion, withPicked } from "./catalog";
import { t } from "../i18n";

interface Props {
  connectionId: string;
  database: string | null | undefined;
  schema: string;
  table: string;
  /** The session that reads the table's columns. */
  sessionId: number;
  onClose: () => void;
}

/** Picks the column that describes the rows of a table beside the foreign keys to it. */
export function DisplayColumnDialog({ connectionId, database, schema, table, sessionId, onClose }: Props) {
  const [columns, setColumns] = useState<string[] | null>(null);
  useCatalogVersion();
  // "" stands for Automatic; until the user picks, the stored choice, which may still be loading.
  const [choice, setChoice] = useState<string | null>(null);
  const picked = choice ?? displayColumn(connectionId, database, schema, table) ?? "";
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api.tableDetails(sessionId, schema, table).then(
      (d) => live && setColumns(d.columns.map((c) => c.name)),
      (e: unknown) => live && setError(toCommandError(e).message),
    );
    return () => {
      live = false;
    };
  }, [sessionId, schema, table]);

  async function save() {
    try {
      await setDisplayColumn(connectionId, database, schema, table, picked || null);
      onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal small"
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <h2>{t("Row label of {table}", { table })}</h2>
        <p className="muted small">{t("The column shown beside the foreign keys that point to this table.")}</p>
        <label className="field">
          <span>{t("Column")}</span>
          <select autoFocus value={picked} onChange={(e) => setChoice(e.target.value)}>
            <option value="">{t("Automatic")}</option>
            {withPicked(columns ?? [], picked).map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" disabled={columns === null}>
            {t("Save")}
          </button>
        </div>
      </form>
    </div>
  );
}
