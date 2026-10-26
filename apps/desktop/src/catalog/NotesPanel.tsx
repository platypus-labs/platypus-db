import { useState } from "react";
import type { CatalogKey } from "../api";
import {
  type CatalogChange,
  catalogEntry,
  displayColumn,
  reportCatalogError,
  saveCatalog,
  setDisplayColumn,
  useCatalogVersion,
  withPicked,
} from "./catalog";
import { TagInput } from "./TagInput";
import { t } from "../i18n";

interface Props {
  connectionId: string;
  catalogKey: CatalogKey;
  /** Tags already used on this connection. */
  suggestions: string[];
  /** A table's columns, to pick the one that describes its rows; absent for other objects. */
  columns?: string[];
}

/** Saves `change` to the note of `key`; resolves whether it was saved, after reporting a failure. */
const saveNote = (connectionId: string, key: CatalogKey, change: CatalogChange): Promise<boolean> =>
  saveCatalog(connectionId, key, change).then(
    () => true,
    (e: unknown) => {
      reportCatalogError(e);
      return false;
    },
  );

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

/** The stored tags of `catalogKey`, saved as they change. It reads the catalog as it draws: the
 *  caller re-renders it on `useCatalogVersion`. */
export function CatalogTags({
  connectionId,
  catalogKey,
  suggestions,
  label,
}: {
  connectionId: string;
  catalogKey: CatalogKey;
  suggestions: string[];
  label?: string;
}) {
  return (
    <TagInput
      label={label}
      tags={catalogEntry(connectionId, catalogKey)?.tags ?? []}
      suggestions={suggestions}
      onChange={(update) => saveNote(connectionId, catalogKey, { tags: update })}
    />
  );
}

/** The note and tags of an object, saved when a field is left. */
export function NotesPanel({ connectionId, catalogKey, suggestions, columns }: Props) {
  useCatalogVersion();
  const note = useNoteDraft(connectionId, catalogKey);

  const { database, schema, object } = catalogKey;
  const display = displayColumn(connectionId, database, schema, object) ?? "";

  return (
    <div className={columns ? "notes-panel with-label" : "notes-panel"}>
      <textarea aria-label={t("Note")} placeholder={t("Note (only on this computer)")} rows={1} {...note} />
      <CatalogTags connectionId={connectionId} catalogKey={catalogKey} suggestions={suggestions} />
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
