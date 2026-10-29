import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type ExportFormat, type ExportOptions, type SavedConnection, type TransferProfile } from "../api";
import { CsvOptions, FORMATS, OutputOptions, sentOptions } from "../components/ExportDialog";
import { workbench } from "../workbench";
import { answerFor } from "../testAnswers";
import { JobFooter, useJob } from "./job";
import { existingTables, ProfileButtons } from "./ProfileSave";
import { t, plural, msg } from "../i18n";

type TableRef = { schema: string | null; name: string };

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  /** Whether `schema` is the session's current one, whose tables are named unqualified. */
  currentSchema: boolean;
  /** The tables to export, each with its schema (`null` for the current one); `null` for
   *  every table of `schema`. */
  tables: TableRef[] | null;
  /** The profile the dialog was opened from: its options start the dialog, and its tables
   *  are checked against the database. */
  profile?: TransferProfile;
  onClose: () => void;
}

/** Exports several tables, or every table of a schema, to one file each in a folder. */
export function ExportTablesDialog({ connection, database, schema, currentSchema, tables: chosen, profile, onClose }: Props) {
  const saved = profile?.task.kind === "export_tables" ? profile.task : null;
  const [tables, setTables] = useState<TableRef[] | null>(chosen);
  const [options, setOptions] = useState<ExportOptions>(saved?.options ?? { format: "csv", delimiter: ",", header: true });
  /** What the profile names that the database no longer has. */
  const [gone, setGone] = useState<string[]>([]);
  const { status, start, fail } = useJob();

  useEffect(() => {
    if (chosen && profile) {
      void workbench()
        .sideSession(connection, database)
        .then(async (sid) => {
          if (sid === null) return;
          const found = await existingTables(sid, chosen);
          setTables(found.kept);
          setGone(found.gone);
        })
        .catch(fail);
    }
    if (chosen) return;
    void workbench()
      .sideSession(connection, database)
      .then(async (sid) => {
        if (sid === null) return;
        const objects = await api.listObjects(sid, schema);
        const own = currentSchema ? null : schema;
        setTables(objects.filter((o) => o.kind === "table").map((o) => ({ schema: own, name: o.name })));
      })
      .catch(fail);
  }, []);

  const askDir = async () => {
    const dir = await answerFor("folder", () => open({ directory: true, title: t("Export to the folder"), defaultPath: saved?.dir }));
    return typeof dir === "string" ? dir : null;
  };

  async function build() {
    const dir = await askDir();
    return dir && tables ? { kind: "export_tables" as const, connection_id: connection.id, database, tables, dir, options: sentOptions(options) } : null;
  }

  async function run() {
    const dir = await askDir();
    if (dir === null || !tables) return;
    const sid = await workbench().sideSession(connection, database);
    if (sid === null) return;
    await start((onEvent) => api.exportTables(sid, tables, dir, sentOptions(options), onEvent));
  }

  const running = status.state === "running";
  const close = () => !running && onClose();
  const count = tables?.length ?? 0;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal small export-tables" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{chosen ? plural(count, "Export {n} table", "Export {n} tables") : t("Export the tables of {schema}", { schema })}</h2>
        <p className="muted small">
          {tables === null
            ? t("Reading the tables…")
            : t("One file per table, named after it: {names}.", { names: tables.map((table) => table.name).join(", ") || t("no tables") })}
        </p>
        {gone.length > 0 && (
          <div className="notice warn small">{t("No longer in the database, left out: {names}.", { names: gone.join(", ") })}</div>
        )}
        <label className="field">
          <span>{t("Format")}</span>
          <select
            value={options.format}
            disabled={running}
            onChange={(e) => setOptions({ ...options, format: e.target.value as ExportFormat })}
          >
            {FORMATS.map((f) => (
              <option key={f.value} value={f.value}>
                {t(f.label)}
              </option>
            ))}
          </select>
        </label>
        {options.format === "csv" && <CsvOptions options={options} disabled={running} onChange={setOptions} />}
        <OutputOptions options={options} signedUrl={false} disabled={running} onChange={setOptions} />
        <JobFooter status={status} messages={[msg("Exporting… {n} rows"), msg("Exported {n} rows.")]} stop={t("Cancel export")} onClose={onClose}>
          <ProfileButtons profile={profile} build={build} disabled={running || count === 0} />
          <button type="button" className="primary" disabled={running || count === 0} onClick={() => void run()}>
            {t("Export to folder…")}
          </button>
        </JobFooter>
      </div>
    </div>
  );
}
