import { useState } from "react";
import { type CatalogKey, toCommandError } from "../api";
import {
  type CatalogChange,
  catalogEntry,
  displayColumn,
  saveCatalog,
  setDisplayColumn,
  succeeded,
  useCatalogVersion,
  withPicked,
} from "./catalog";
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

export const reportCatalogError = (e: unknown) => workbench().showError(toCommandError(e).message);

/** Saves `change` to the note of `key`; resolves whether it was saved, after reporting a failure. */
export const saveNote = (connectionId: string, key: CatalogKey, change: CatalogChange) =>
  succeeded(saveCatalog(connectionId, key, change), reportCatalogError);

/**
 * The text of a note field: what is typed, until leaving the field saves it, then the stored
 * note. A load or a save landing meanwhile does not replace what is being typed.
 */
export function useNoteDraft(connectionId: string, key: CatalogKey) {
  const stored = catalogEntry(connectionId, key)?.note ?? "";
  const [draft, setDraft] = useState<string | null>(null);
  return {
    value: draft ?? stored,
    onChange: (e: { target: { value: string } }) => setDraft(e.target.value),
    onBlur: () => {
      if (draft === null) return;
      if (draft === stored) {
        setDraft(null);
        return;
      }
      // A failed save keeps the text, to try again.
      void saveNote(connectionId, key, { note: draft }).then((saved) => {
        if (saved) setDraft((now) => (now === draft ? null : now));
      });
    },
  };
}

/** The note and tags of an object, saved when a field is left. */
export function NotesPanel({ connectionId, catalogKey, suggestions, columns }: Props) {
  useCatalogVersion();
  const entry = catalogEntry(connectionId, catalogKey);
  const note = useNoteDraft(connectionId, catalogKey);

  const { database, schema, object } = catalogKey;
  const display = displayColumn(connectionId, database, schema, object) ?? "";

  return (
    <div className={columns ? "notes-panel with-label" : "notes-panel"}>
      <textarea aria-label={t("Note")} placeholder={t("Note (only on this computer)")} rows={1} {...note} />
      <TagInput
        tags={entry?.tags ?? []}
        suggestions={suggestions}
        onChange={(update) => saveNote(connectionId, catalogKey, { tags: update })}
      />
      {columns && (
        <label className="row-label muted small" title={t("The column shown beside the foreign keys that point to this table.")}>
          {t("Row label")}
          <select
            value={display}
            onChange={(e) =>
              void setDisplayColumn(connectionId, database, schema, object, e.target.value || null).catch(reportCatalogError)
            }
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
