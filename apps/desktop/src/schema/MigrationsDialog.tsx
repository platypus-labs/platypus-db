import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type MigrationEntry, type MigrationState, type MigrationToolStatus, type SavedConnection, toCommandError } from "../api";
import { workbench } from "../workbench";
import { t, msg } from "../i18n";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  onClose: () => void;
}

const TOOL_NAMES = { flyway: "Flyway", liquibase: "Liquibase", prisma: "Prisma", alembic: "Alembic", rails: "Rails" } as const;

const STATUS_LABELS: Record<MigrationState, string> = {
  applied: msg("Applied"),
  pending: msg("Pending"),
  divergent: msg("Divergent"),
  failed: msg("Failed"),
  missing: msg("Missing"),
};
const STATES = Object.keys(STATUS_LABELS) as MigrationState[];
const PROBLEMS: MigrationState[] = ["pending", "divergent", "failed", "missing"];

// The details the backend writes in English, translated where they are shown.
const DETAILS: Record<string, string> = {
  "out of order": msg("out of order"),
  baseline: msg("baseline"),
  "below the baseline": msg("below the baseline"),
  "no file in the folder": msg("no file in the folder"),
  "rolled back": msg("rolled back"),
  "changed, will be applied again": msg("changed, will be applied again"),
  head: msg("head"),
  "branch head": msg("branch head"),
  skipped: msg("skipped"),
  mark_ran: msg("marked as run"),
  reran: msg("run again"),
};
msg("checksum differs (database {database}, file {file})");

function detailText(detail: string | null): string {
  if (!detail) return "";
  const checksum = /^checksum differs \(database (.*), file (.*)\)$/.exec(detail);
  if (checksum) return t("checksum differs (database {database}, file {file})", { database: checksum[1] ?? "", file: checksum[2] ?? "" });
  return DETAILS[detail] ? t(DETAILS[detail]) : detail;
}

const folderKey = (connection: SavedConnection) => `platypus.migrations.folder.${connection.id}`;

/** The folder last checked on this connection, kept in the web view's local storage. */
function savedFolder(connection: SavedConnection): string {
  try {
    return localStorage.getItem(folderKey(connection)) ?? "";
  } catch {
    return "";
  }
}

function saveFolder(connection: SavedConnection, folder: string) {
  try {
    localStorage.setItem(folderKey(connection), folder);
  } catch {
    // The folder is just not remembered.
  }
}

/** Compares the migrations of a project folder (Flyway, Liquibase, Prisma, Alembic, Rails)
 *  with the history tables of the schema: applied, pending, changed since they were applied,
 *  failed, or recorded in the database with no file. A `.sql` migration opens in the editor. */
export function MigrationsDialog({ connection, database, schema, onClose }: Props) {
  const [folder, setFolder] = useState(() => savedFolder(connection));
  const [result, setResult] = useState<MigrationToolStatus[] | null>(null);
  const [checked, setChecked] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [onlyProblems, setOnlyProblems] = useState(false);

  useEffect(() => {
    if (folder.trim() !== "") void check();
  }, []);

  async function browse() {
    const picked = await open({ directory: true, multiple: false, title: t("Choose the migrations folder"), defaultPath: folder || undefined });
    if (typeof picked === "string") setFolder(picked);
  }

  async function check() {
    const path = folder.trim();
    if (path === "") return;
    setBusy(true);
    setError(null);
    try {
      const sid = await workbench().sideSession(connection, database);
      if (sid === null) return onClose();
      const found = await api.migrationStatus(sid, schema, path);
      saveFolder(connection, path);
      setResult(found);
      setChecked(path);
    } catch (e) {
      setResult(null);
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function openFile(m: MigrationEntry) {
    if (!m.file || !m.file.toLowerCase().endsWith(".sql")) return;
    try {
      const sql = await api.readTextFile(`${checked.replace(/[\\/]+$/, "")}/${m.file}`);
      workbench().openTab("query", connection.id, { sql, database });
      onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  const all = (result ?? []).flatMap((r) => r.migrations);
  const count = (s: MigrationState) => all.filter((m) => m.status === s).length;

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="modal schema-migrations" onKeyDown={(e) => e.key === "Escape" && !busy && onClose()}>
        <h2>{t("Migration status of {schema}", { schema })}</h2>
        <p className="muted small">
          {t("The Flyway, Liquibase, Prisma, Alembic and Rails migrations in the folder are compared with the history tables of the database.")}
        </p>
        <div className="row">
          <label className="field grow">
            <span>{t("Migrations folder")}</span>
            <input
              data-field="folder"
              value={folder}
              disabled={busy}
              spellCheck={false}
              onChange={(e) => setFolder(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void check()}
            />
          </label>
          <button type="button" data-action="browse" disabled={busy} onClick={() => void browse()}>
            {t("Browse…")}
          </button>
          <button type="button" className="primary" data-action="check" disabled={busy || folder.trim() === ""} onClick={() => void check()}>
            {busy ? t("Checking…") : t("Check")}
          </button>
        </div>
        {error && <div className="notice error small" data-field="error">{error}</div>}
        {result !== null && (
          <>
            {all.length === 0 ? (
              <p className="muted small" data-field="empty">{t("No migrations found in the folder or in the database.")}</p>
            ) : (
              <div className="migration-summary">
                {STATES.map((s) => (
                  <span key={s} className={`mig-status ${s}`} data-count={s}>
                    {t(STATUS_LABELS[s])} {count(s)}
                  </span>
                ))}
                <span className="spacer" />
                <label className="inline">
                  <input type="checkbox" data-field="only-problems" checked={onlyProblems} onChange={(e) => setOnlyProblems(e.target.checked)} />{" "}
                  {t("Only problems")}
                </label>
              </div>
            )}
            <div className="migration-tools">
              {result.map((r) => (
                <ToolTable key={r.tool} status={r} onlyProblems={onlyProblems} onOpen={(m) => void openFile(m)} />
              ))}
            </div>
          </>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" disabled={busy} onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}

function ToolTable({ status, onlyProblems, onOpen }: { status: MigrationToolStatus; onlyProblems: boolean; onOpen: (m: MigrationEntry) => void }) {
  const rows = status.migrations.filter((m) => !onlyProblems || PROBLEMS.includes(m.status));
  return (
    <section data-tool={status.tool}>
      <h3>
        {TOOL_NAMES[status.tool]}
        <span className="muted small">
          {" · "}
          {status.history_table ? status.history_table : t("no history table: nothing applied")}
        </span>
      </h3>
      {rows.length === 0 ? (
        <p className="muted small">{t("Nothing to show.")}</p>
      ) : (
        <table className="meta-table migration-table">
          <thead>
            <tr>
              <th>{t("Version")}</th>
              <th>{t("Description")}</th>
              <th>{t("File")}</th>
              <th>{t("Status")}</th>
              <th>{t("Applied at")}</th>
              <th>{t("Detail")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((m, i) => (
              <tr key={i} data-id={m.id} data-status={m.status} onDoubleClick={() => onOpen(m)}>
                <td>{m.id}</td>
                <td>{m.description}</td>
                <td>{m.file ?? ""}</td>
                <td>
                  <span className={`mig-status ${m.status}`}>{t(STATUS_LABELS[m.status])}</span>
                </td>
                <td>{m.applied_at ?? ""}</td>
                <td>{detailText(m.detail)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
