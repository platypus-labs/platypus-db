import { useEffect, useRef, useState } from "react";
import { api, type SavedConnection, toCommandError } from "../api";
import { workbench } from "../workbench";
import { t } from "../i18n";

export interface SqlActionOptions {
  /** Shown as a CASCADE checkbox when the dialect supports it. */
  cascade?: boolean;
  /** Asks for a new name, starting from this one. */
  rename?: string;
  /** Label of the text field when it is not a name. */
  renameLabel?: string;
  /** The text may be emptied (clearing a comment). */
  allowEmpty?: boolean;
  /** The text may span lines (a comment): Enter starts a new line, Ctrl+Enter runs. */
  multiline?: boolean;
}

interface Choices {
  cascade: boolean;
  newName: string;
}

interface Props {
  connection: SavedConnection;
  database: string | null;
  title: string;
  /** What the statement does, in a sentence. */
  description: string;
  /** Primary button label, e.g. "Drop". */
  confirm: string;
  /** Whether the statement destroys something; the primary button then says so. Default true. */
  danger?: boolean;
  options: SqlActionOptions;
  /** The statement for the current choices; `null` when they produce none. */
  generate: (choices: Choices) => Promise<string | null>;
  /** Called after the statement ran. */
  onDone: (choices: Choices) => void;
  onClose: () => void;
}

/** How long typing pauses before the statement is generated again. */
const TYPING_PAUSE_MS = 250;

/** Shows the statement an object action will run, lets the user adjust it, then runs it
 *  on the navigator session. */
export function SqlActionDialog(props: Props) {
  const { connection, database, options } = props;
  const [cascade, setCascade] = useState(false);
  const [newName, setNewName] = useState(options.rename ?? "");
  /** The last statement generated, with the choices it was generated for. */
  const [preview, setPreview] = useState<(Choices & { sql: string | null }) | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  // The first statement comes at once; while typing, after a pause.
  const opened = useRef(true);
  const renaming = options.rename !== undefined;
  const unchanged = renaming && ((newName.trim() === "" && !options.allowEmpty) || newName === options.rename);
  const choices: Choices = { cascade, newName: newName.trim() };
  // Only the statement of what the dialog shows now runs: one still being generated for an
  // earlier keystroke does not.
  const fresh = !unchanged && preview?.cascade === choices.cascade && preview.newName === choices.newName;
  const sql = fresh ? preview.sql : null;

  useEffect(() => {
    if (unchanged) {
      setPreview(null);
      return;
    }
    let current = true;
    const timer = window.setTimeout(
      () =>
        props.generate(choices).then(
          (s) => {
            if (!current) return;
            setPreview({ ...choices, sql: s });
            setError(null);
          },
          (e: unknown) => current && setError(toCommandError(e).message),
        ),
      opened.current ? 0 : TYPING_PAUSE_MS,
    );
    opened.current = false;
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [cascade, newName]);

  async function run() {
    if (!sql || running || connection.read_only) return;
    setRunning(true);
    setError(null);
    try {
      const sid = await workbench().sideSession(connection, database);
      if (sid === null) return;
      await api.executeStatement(sid, sql);
      props.onDone(choices);
      props.onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setRunning(false);
    }
  }

  const field = {
    autoFocus: true,
    value: newName,
    onChange: (e: { target: { value: string } }) => setNewName(e.target.value),
  };
  const hint = options.renameLabel ? t("Change the text.") : t("Type a new name.");

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
            <span>{options.renameLabel ?? t("New name")}</span>
            {options.multiline ? (
              <textarea
                {...field}
                rows={4}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                    e.preventDefault();
                    e.currentTarget.form?.requestSubmit();
                  }
                }}
              />
            ) : (
              <input {...field} />
            )}
          </label>
        )}
        {options.cascade && (
          <label className="check">
            <input type="checkbox" checked={cascade} onChange={(e) => setCascade(e.target.checked)} />
            <span>{t("CASCADE: also drop the objects that depend on it")}</span>
          </label>
        )}
        <pre className="sql-preview">{unchanged ? hint : (preview?.sql ?? "…")}</pre>
        {connection.read_only && (
          <div className="notice warn small">{t("{name} is read-only: this statement cannot be run.", { name: connection.name })}</div>
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
            {t("Open in editor")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={props.onClose}>
            {t("Cancel")}
          </button>
          <button
            type="submit"
            className={props.danger === false ? "primary" : "primary danger"}
            autoFocus={!renaming}
            disabled={!sql || running || connection.read_only}
          >
            {running ? t("Running…") : props.confirm}
          </button>
        </div>
      </form>
    </div>
  );
}
