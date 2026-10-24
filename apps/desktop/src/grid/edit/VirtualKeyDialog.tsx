import { useState } from "react";
import { api, type SavedConnection, toCommandError } from "../../api";
import { workbench } from "../../workbench";
import type { TableRef } from "./meta";

interface Props {
  connection: SavedConnection;
  table: TableRef;
  /** The table's columns, in order. */
  columns: string[];
  /** The current virtual key, if any. */
  current: string[] | null;
  onClose: () => void;
}

/** Picks the columns that identify the rows of a table without a primary key. */
export function VirtualKeyDialog({ connection, table, columns, current, onClose }: Props) {
  const [picked, setPicked] = useState(() => new Set(current ?? []));
  const [error, setError] = useState<string | null>(null);

  async function save(names: string[]) {
    try {
      workbench().setConnections(await api.setVirtualKey(connection.id, table.schema, table.name, names));
      onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  function toggle(name: string) {
    setPicked((p) => {
      const next = new Set(p);
      if (!next.delete(name)) next.add(name);
      return next;
    });
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal small" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>Virtual key of {table.name}</h2>
        <p className="muted small">
          The columns whose values tell the rows apart. Saving checks that every change hits exactly one row,
          so a key that is not unique cannot change other rows.
        </p>
        <div className="virtual-key-columns">
          {columns.map((name) => (
            <label key={name} className="check">
              <input type="checkbox" checked={picked.has(name)} onChange={() => toggle(name)} />
              {name}
            </label>
          ))}
        </div>
        {error && <p className="error-text small">{error}</p>}
        <div className="actions">
          {current && (
            <button type="button" onClick={() => void save([])}>
              Remove
            </button>
          )}
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className="primary"
            disabled={picked.size === 0}
            // In table order, whatever the order of the clicks.
            onClick={() => void save(columns.filter((c) => picked.has(c)))}
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
