import { createElement, useState } from "react";
import { toCommandError } from "../api";
import { workbench } from "../workbench";

interface Props {
  title: string;
  initial: string;
  /** Saves the name; a thrown error is shown and the dialog stays open. */
  onSave: (name: string) => Promise<void>;
  onClose: () => void;
}

function NameDialog({ title, initial, onSave, onClose }: Props) {
  const [name, setName] = useState(initial);
  const [error, setError] = useState<string | null>(null);

  async function save() {
    try {
      await onSave(name.trim());
      onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal name-dialog"
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <h2>{title}</h2>
        <label className="field">
          <span>Name</span>
          <input autoFocus value={name} onFocus={(e) => e.target.select()} onChange={(e) => setName(e.target.value)} />
        </label>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={!name.trim()}>
            Save
          </button>
        </div>
      </form>
    </div>
  );
}

/** Asks for a name in a small dialog and passes it to `onSave`. */
export function askName(title: string, initial: string, onSave: (name: string) => Promise<void>) {
  workbench().openDialog((close) => createElement(NameDialog, { title, initial, onSave, onClose: close }));
}
