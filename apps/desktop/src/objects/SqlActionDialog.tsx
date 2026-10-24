import { useEffect, useState } from "react";
import { api, type SavedConnection, toCommandError } from "../api";
import { workbench } from "../workbench";

export interface SqlActionOptions {
  /** Shown as a CASCADE checkbox when the dialect supports it. */
  cascade?: boolean;
  /** Asks for a new name, starting from this one. */
  rename?: string;
}

interface Props {
  connection: SavedConnection;
  database: string | null;
  title: string;
  /** What the statement does, in a sentence. */
  description: string;
  /** Primary button label, e.g. "Drop". */
  confirm: string;
  options: SqlActionOptions;
  /** The statement for the current choices; `null` when they produce none. */
  generate: (choices: { cascade: boolean; newName: string }) => Promise<string | null>;
  /** Called after the statement ran. */
  onDone: () => void;
  onClose: () => void;
}

/** Shows the statement an object action will run, lets the user adjust it, then runs it
 *  on the navigator session. */
export function SqlActionDialog(props: Props) {
  const { connection, database, options } = props;
  const [cascade, setCascade] = useState(false);
  const [newName, setNewName] = useState(options.rename ?? "");
  const [sql, setSql] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const renaming = options.rename !== undefined;
  const unchanged = renaming && (newName.trim() === "" || newName === options.rename);

  useEffect(() => {
    let current = true;
    if (unchanged) {
      setSql(null);
      return;
    }
    props.generate({ cascade, newName: newName.trim() }).then(
      (s) => current && setSql(s),
      (e: unknown) => current && setError(toCommandError(e).message),
    );
    return () => {
      current = false;
    };
  }, [cascade, newName]);

  async function run() {
    if (!sql) return;
    setRunning(true);
    setError(null);
    try {
      const sid = await workbench().sideSession(connection, database);
      if (sid === null) return;
      await api.executeStatement(sid, sql);
      props.onDone();
      props.onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={props.onClose}>
      <form
        className="modal sql-action"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && props.onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <h2>{props.title}</h2>
        <p className="muted small">{props.description}</p>
        {renaming && (
          <label className="field">
            <span>New name</span>
            <input autoFocus value={newName} onChange={(e) => setNewName(e.target.value)} />
          </label>
        )}
        {options.cascade && (
          <label className="check">
            <input type="checkbox" checked={cascade} onChange={(e) => setCascade(e.target.checked)} />
            <span>CASCADE: also drop the objects that depend on it</span>
          </label>
        )}
        <pre className="sql-preview">{sql ?? (unchanged ? "Type a new name." : "…")}</pre>
        {connection.read_only && (
          <div className="notice warn small">{connection.name} is read-only: this statement cannot be run.</div>
        )}
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <button
            type="button"
            className="link"
            disabled={!sql}
            onClick={() => {
              if (sql) workbench().openTab("query", connection.id, { sql, database });
              props.onClose();
            }}
          >
            Open in editor
          </button>
          <span className="spacer" />
          <button type="button" onClick={props.onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="primary danger"
            autoFocus={!renaming}
            disabled={!sql || running || connection.read_only}
          >
            {running ? "Running…" : props.confirm}
          </button>
        </div>
      </form>
    </div>
  );
}
