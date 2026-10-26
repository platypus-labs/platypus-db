import { useEffect, useState } from "react";
import { type CatalogKey, toCommandError } from "../api";
import { catalogEntry, displayColumn, saveCatalog, setDisplayColumn, succeeded, useCatalogVersion, withPicked } from "./catalog";
import { TagInput } from "./TagInput";
import { workbench } from "../workbench";
import { t } from "../i18n";

interface Props {
  connectionId: string;
  catalogKey: CatalogKey;
  /** Tags already used on this connection. */
  suggestions: string[];
  /** A table's columns, to pick the one that describes its rows; absent for other objects. */
  columns?: string[];
}

/** The note and tags of an object, saved when a field is left. */
export function NotesPanel({ connectionId, catalogKey, suggestions, columns }: Props) {
  useCatalogVersion();
  const entry = catalogEntry(connectionId, catalogKey);
  const [note, setNote] = useState(entry?.note ?? "");
  useEffect(() => setNote(entry?.note ?? ""), [entry?.note]);

  const { database, schema, object } = catalogKey;
  const display = displayColumn(connectionId, database, schema, object) ?? "";

  const failed = (e: unknown) => workbench().showError(toCommandError(e).message);
  const save = (n: string, tags: string[]) => succeeded(saveCatalog(connectionId, catalogKey, n, tags), failed);

  return (
    <div className={columns ? "notes-panel with-label" : "notes-panel"}>
      <textarea
        aria-label={t("Note")}
        placeholder={t("Note (only on this computer)")}
        value={note}
        rows={1}
        onChange={(e) => setNote(e.target.value)}
        onBlur={() => note !== (entry?.note ?? "") && void save(note, entry?.tags ?? [])}
      />
      <TagInput tags={entry?.tags ?? []} suggestions={suggestions} onChange={(tags) => save(note, tags)} />
      {columns && (
        <label className="row-label muted small" title={t("The column shown beside the foreign keys that point to this table.")}>
          {t("Row label")}
          <select
            value={display}
            onChange={(e) => void setDisplayColumn(connectionId, database, schema, object, e.target.value || null).catch(failed)}
          >
            <option value="">{t("Automatic")}</option>
            {withPicked(columns, display).map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>
      )}
    </div>
  );
}
