import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type PlanSource, type ReadPlan, toCommandError } from "../api";
import { msg, t } from "../i18n";

const SOURCES: [PlanSource | "", string][] = [
  ["", msg("Recognize it")],
  ["postgres", msg("PostgreSQL (JSON or text), CockroachDB")],
  ["mysql", msg("MySQL (tree), MariaDB (JSON)")],
  ["sqlite", msg("SQLite (QUERY PLAN)")],
  ["sqlserver", msg("SQL Server (SHOWPLAN_TEXT)")],
];

/** Reads a plan pasted or opened from a file, e.g. from a server's log or a colleague, and
 *  hands it over with a name: the file's, or "Pasted plan". */
export function PlanDialog({
  title,
  onRead,
  onClose,
}: {
  title: string;
  onRead: (plan: ReadPlan, name: string) => void;
  onClose: () => void;
}) {
  const [text, setText] = useState("");
  const [file, setFile] = useState<string | null>(null);
  const [source, setSource] = useState<PlanSource | "">("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function openFile() {
    const path = await open({
      filters: [{ name: t("Query plans"), extensions: ["json", "txt", "log", "sql", "plan"] }],
      multiple: false,
    });
    if (typeof path !== "string") return;
    try {
      setText(await api.readTextFile(path));
      setFile(path.split(/[\\/]/).pop() ?? path);
      setError(null);
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  async function read() {
    setBusy(true);
    setError(null);
    try {
      const plan = await api.readPlan(text, source || null);
      onRead(plan, file ?? t("Pasted plan"));
      onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <form
        className="modal plan-dialog"
        onKeyDown={(e) => e.key === "Escape" && !busy && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          void read();
        }}
      >
        <h2>{title}</h2>
        <p className="muted small">
          {t("Paste the output of EXPLAIN, as psql, the MySQL client, sqlcmd or a server log prints it, or open a file.")}
        </p>
        <textarea
          className="plan-text"
          autoFocus
          spellCheck={false}
          value={text}
          placeholder={t("Paste the plan here")}
          onChange={(e) => {
            setText(e.target.value);
            setFile(null);
          }}
          aria-label={t("Query plan")}
        />
        <div className="row">
          <label className="field grow">
            <span>{t("Database")}</span>
            <select value={source} onChange={(e) => setSource(e.target.value as PlanSource | "")}>
              {SOURCES.map(([value, label]) => (
                <option key={value} value={value}>
                  {t(label)}
                </option>
              ))}
            </select>
          </label>
        </div>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <button type="button" disabled={busy} onClick={() => void openFile()}>
            {t("Open file…")}
          </button>
          {file && <span className="muted small plan-file">{file}</span>}
          <span className="spacer" />
          <button type="button" disabled={busy} onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" disabled={busy || !text.trim()}>
            {t("Open")}
          </button>
        </div>
      </form>
    </div>
  );
}
