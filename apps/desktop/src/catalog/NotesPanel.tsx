import { useEffect, useState } from "react";
import { type CatalogKey, toCommandError } from "../api";
import { catalogEntry, saveCatalog, useCatalogVersion } from "./catalog";
import { TagInput } from "./TagInput";
import { workbench } from "../workbench";

interface Props {
  connectionId: string;
  catalogKey: CatalogKey;
  /** Tags already used on this connection. */
  suggestions: string[];
}

/** The note and tags of an object, saved when a field is left. */
export function NotesPanel({ connectionId, catalogKey, suggestions }: Props) {
  useCatalogVersion();
  const entry = catalogEntry(connectionId, catalogKey);
  const [note, setNote] = useState(entry?.note ?? "");
  useEffect(() => setNote(entry?.note ?? ""), [entry?.note]);

  const save = (n: string, tags: string[]) =>
    saveCatalog(connectionId, catalogKey, n, tags).catch((e: unknown) => workbench().showError(toCommandError(e).message));

  return (
    <div className="notes-panel">
      <textarea
        aria-label="Note"
        placeholder="Note (only on this computer)"
        value={note}
        rows={1}
        onChange={(e) => setNote(e.target.value)}
        onBlur={() => note !== (entry?.note ?? "") && void save(note, entry?.tags ?? [])}
      />
      <TagInput tags={entry?.tags ?? []} suggestions={suggestions} onChange={(tags) => void save(note, tags)} />
    </div>
  );
}
