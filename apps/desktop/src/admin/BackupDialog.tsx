/** "Back up…" and "Restore…" of a PostgreSQL or MySQL database, with the database's own tools
 *  (`pg_dump`, `pg_restore`, `psql`, `mysqldump`, `mysql`) found on the PATH: the file and
 *  its options, the progress table by table (or by the bytes of a SQL file), and what the
 *  tool said. */
import { open, save } from "@tauri-apps/plugin-dialog";
import { useRef, useState } from "react";
import { api, type BackupEvent, type BackupFormat, type SavedConnection, toCommandError } from "../api";
import { datedFileName, formatBytes } from "../format";
import { formatCount, t } from "../i18n";
import { answerFor } from "../testAnswers";
import { ask } from "../ui/ask";
import { workbench } from "../workbench";
import { duration } from "./sessions";

export type BackupMode = "backup" | "restore";

interface Props {
  mode: BackupMode;
  connection: SavedConnection;
  database: string | null;
  /** Only this schema (PostgreSQL); the whole database otherwise. */
  schema: string | null;
  onClose: () => void;
}

type Status =
  | { state: "idle" }
  | { state: "running"; step: string; done: number; total: number | null; bytes: boolean }
  | { state: "finished"; bytes: number; ms: number; output: string }
  | { state: "failed"; message: string; output: string };

/** The run ids of the tools, as the terminal's are: unique in the app's lifetime. */
let runs = Date.now();

export function BackupDialog({ mode, connection, database, schema, onClose }: Props) {
  const postgres = connection.driver === "postgres";
  const target = schema ?? database ?? connection.database ?? connection.name;
  const [format, setFormat] = useState<BackupFormat>(postgres ? "custom" : "plain");
  const [structureOnly, setStructureOnly] = useState(false);
  const [clean, setClean] = useState(false);
  const [path, setPath] = useState("");
  const [status, setStatus] = useState<Status>({ state: "idle" });
  const runId = useRef<number | null>(null);
  const running = status.state === "running";
  const close = () => !running && onClose();
  const ext = format === "custom" ? "dump" : "sql";

  async function choose() {
    const chosen = await answerFor("file", () =>
      mode === "backup"
        ? save({
            defaultPath: datedFileName(target, ext),
            filters: [{ name: format === "custom" ? t("Archive") : "SQL", extensions: [ext] }],
          })
        : open({ multiple: false, filters: [{ name: t("Backups"), extensions: ["dump", "backup", "sql"] }] }),
    );
    if (typeof chosen === "string") setPath(chosen);
  }

  function onEvent(e: BackupEvent) {
    if (e.kind === "progress") setStatus({ state: "running", step: e.step, done: e.done, total: e.total, bytes: e.bytes });
    else if (e.kind === "finished") setStatus({ state: "finished", bytes: e.bytes, ms: e.elapsed_ms, output: e.output });
    else setStatus({ state: "failed", message: e.message, output: e.output });
  }

  async function run() {
    if (
      mode === "restore" &&
      !(await ask(t("Restore {file} into {database}? Its statements change the database.", { file: path, database: target }), {
        confirm: t("Restore"),
        danger: true,
      }))
    )
      return;
    const id = ++runs;
    runId.current = id;
    setStatus({ state: "running", step: "", done: 0, total: null, bytes: false });
    try {
      const sid = await workbench().sideSession(connection, database);
      if (sid === null) return setStatus({ state: "idle" });
      if (mode === "backup") {
        await api.backupDatabase(sid, id, { path: path.trim(), format, schema, structure_only: structureOnly }, onEvent);
      } else {
        const restore = (allow: boolean) =>
          api.restoreDatabase(sid, id, { path: path.trim(), clean, allow_client_commands: allow }, onEvent);
        try {
          await restore(false);
        } catch (e) {
          const error = toCommandError(e);
          if (error.kind !== "client_commands") throw e;
          // The file runs commands on this computer, not only on the server: the user sees them first.
          const tool = postgres ? "psql" : "mysql";
          const message = [
            t("This file has commands for {tool} itself, which can run programs and write files on this computer:", { tool }),
            error.message,
            t("Restore the file with these commands?"),
          ].join("\n\n");
          if (!(await ask(message, { confirm: t("Run these commands"), danger: true }))) return setStatus({ state: "idle" });
          await restore(true);
        }
      }
    } catch (e) {
      const error = toCommandError(e);
      setStatus(error.kind === "cancelled" ? { state: "idle" } : { state: "failed", message: error.message, output: "" });
    } finally {
      if (runId.current === id) runId.current = null;
    }
  }

  function cancel() {
    if (runId.current !== null) void api.cancelCli(runId.current);
  }

  const title =
    mode === "backup"
      ? t("Back up {database}", { database: target })
      : t("Restore into {database}", { database: target });
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <form
        className="modal backup-dialog"
        data-mode={mode}
        onKeyDown={(e) => e.key === "Escape" && close()}
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <h2>{title}</h2>
        <p className="muted small">
          {mode === "backup"
            ? t("Runs {tool} from the PATH, with this connection's settings and password.", {
                tool: postgres ? "pg_dump" : "mysqldump",
              })
            : postgres
              ? t("An archive is restored with pg_restore, a SQL file with psql in one transaction.")
              : t("Runs the SQL file with mysql; views, triggers and routines are made by this connection's user.")}
        </p>
        {mode === "backup" && postgres && (
          <label className="field">
            <span>{t("Format")}</span>
            <select
              value={format}
              disabled={running}
              data-field="format"
              onChange={(e) => {
                const next = e.target.value as BackupFormat;
                setFormat(next);
                // The file keeps its name with the format's extension.
                setPath((p) => p.replace(/\.(dump|sql)$/i, next === "custom" ? ".dump" : ".sql"));
              }}
            >
              <option value="custom">{t("Archive for pg_restore (.dump)")}</option>
              <option value="plain">{t("SQL statements (.sql)")}</option>
            </select>
          </label>
        )}
        <div className="field">
          <span>{t("File")}</span>
          <div className="row tight">
            <input
              aria-label={t("File")}
              data-field="path"
              value={path}
              disabled={running}
              spellCheck={false}
              onChange={(e) => setPath(e.target.value)}
            />
            <button type="button" disabled={running} onClick={() => void choose()}>
              {t("Choose file…")}
            </button>
          </div>
        </div>
        {mode === "backup" ? (
          <label className="check">
            <input
              type="checkbox"
              checked={structureOnly}
              disabled={running}
              data-field="structure_only"
              onChange={(e) => setStructureOnly(e.target.checked)}
            />
            {t("Structure only, without the rows")}
          </label>
        ) : (
          postgres && (
            <label className="check">
              <input type="checkbox" checked={clean} disabled={running} data-field="clean" onChange={(e) => setClean(e.target.checked)} />
              {t("Drop the archive's objects first (archives only)")}
            </label>
          )
        )}
        {status.state === "running" && (
          <div className="docs-progress" data-field="progress">
            <progress max={status.total || undefined} value={status.total ? status.done : undefined} />
            <p className="muted small">{progressText(status)}</p>
          </div>
        )}
        {status.state === "finished" && (
          <div className="notice ok small" data-field="result">
            {mode === "backup"
              ? t("Backup written: {size} in {time}.", { size: formatBytes(status.bytes), time: duration(status.ms / 1000) })
              : t("Restored in {time}.", { time: duration(status.ms / 1000) })}
          </div>
        )}
        {status.state === "failed" && (
          <div className="notice error small" data-field="result">
            {status.message}
          </div>
        )}
        {(status.state === "finished" || status.state === "failed") && status.output.trim() && (
          <details className="backup-output">
            <summary className="muted small">{t("What the tool said")}</summary>
            <pre className="admin-statement">{status.output}</pre>
          </details>
        )}
        <div className="actions">
          <span className="spacer" />
          {running ? (
            <button type="button" onClick={cancel}>
              {t("Cancel")}
            </button>
          ) : (
            <button type="button" onClick={onClose}>
              {t(status.state === "finished" ? "Close" : "Cancel")}
            </button>
          )}
          <button type="submit" className="primary" disabled={running || !path.trim()}>
            {t(mode === "backup" ? "Back up" : "Restore")}
          </button>
        </div>
      </form>
    </div>
  );
}

function progressText(status: Extract<Status, { state: "running" }>): string {
  if (status.bytes) {
    return status.total
      ? t("{done} of {total} sent", { done: formatBytes(status.done), total: formatBytes(status.total) })
      : formatBytes(status.done);
  }
  if (status.done === 0) return t("Starting…");
  const count = status.total
    ? t("{done} of {total} tables", { done: formatCount(status.done), total: formatCount(status.total) })
    : t("{done} tables", { done: formatCount(status.done) });
  return status.step ? `${count} · ${status.step}` : count;
}
