import { useEffect, useRef, useState } from "react";
import { api, isNoSql, opensDataFile, type SavedConnection, type SchemaMeta, takesNoRows, type TransferProfile } from "../api";
import { GuardDialog } from "../guards/GuardDialog";
import { type GuardRequest, needsProductionConfirm } from "../guards/guards";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { workbench } from "../workbench";
import { JobFooter, useJob } from "./job";
import { existingTables, ProfileButtons } from "./ProfileSave";
import { t, plural, msg } from "../i18n";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  /** The tables to copy; `null` for every table of the schema. */
  tables: string[] | null;
  /** Also the schema's views and routines; by default when `tables` is `null`. */
  objects?: boolean;
  /** The profile the dialog was opened from: its target starts the dialog, and its tables
   *  are checked against the database. */
  profile?: TransferProfile;
  onClose: () => void;
}

/** Copies tables, structure and rows, to a schema of another database: each is created with
 *  its columns mapped, NOT NULL, primary key and identity, then the defaults, indexes, foreign
 *  keys and CHECKs follow; a whole schema brings its views and routines too. */
export function MigrateDialog({ connection, database, schema, tables: chosen, objects, profile, onClose }: Props) {
  const saved = profile?.task.kind === "migrate" ? profile.task : null;
  const whole = objects ?? chosen === null;
  const candidates = workbench()
    .connections()
    .filter((c) => c.id !== connection.id && !opensDataFile(c) && !isNoSql(c.driver));
  // Read-only connections, and those whose rows change only by SQL, are listed but take
  // no tables.
  const targets = candidates.filter((c) => !takesNoRows(c));
  const [tables, setTables] = useState<string[] | null>(chosen);
  const [target, setTarget] = useState<SavedConnection | null>(
    (saved && targets.find((c) => c.id === saved.target_connection_id)) || (targets[0] ?? null),
  );
  /** What the profile names that is not there any more. */
  const [gone, setGone] = useState<string[]>([]);
  /** The target schema of the profile, taken once the target's schemas are known. */
  const wantedSchema = useRef(saved?.target_schema ?? null);
  const [schemas, setSchemas] = useState<SchemaMeta[]>([]);
  const [targetSchema, setTargetSchema] = useState("");
  const [sids, setSids] = useState<{ source: number; target: number } | null>(null);
  const { status, start, fail } = useJob();
  const [confirm, setConfirm] = useState<GuardRequest | null>(null);

  useEffect(() => {
    if (chosen && profile) {
      void workbench()
        .sideSession(connection, database)
        .then(async (sid) => {
          if (sid === null) return;
          const found = await existingTables(sid, chosen.map((name) => ({ schema, name })));
          setTables(found.kept.map((x) => x.name));
          setGone(found.gone);
        })
        .catch(fail);
    }
    if (chosen) return;
    workbench()
      .sideSession(connection, database)
      .then(async (sid) => {
        if (sid === null) return;
        const objects = await api.listObjects(sid, schema);
        setTables(objects.filter((o) => o.kind === "table").map((o) => o.name));
      })
      .catch(fail);
  }, []);

  useEffect(() => {
    setSids(null);
    setSchemas([]);
    if (!target) return;
    void (async () => {
      try {
        const source = await workbench().sideSession(connection, database);
        const to = await workbench().sideSession(target, null);
        if (source === null || to === null) return;
        const all = await api.listSchemas(to);
        setSchemas(all);
        const wanted = all.find((s) => s.name === wantedSchema.current);
        if (wantedSchema.current && !wanted) setGone((g) => [...g, wantedSchema.current!]);
        wantedSchema.current = null;
        setTargetSchema((wanted ?? all.find((s) => s.current) ?? all[0])?.name ?? "");
        setSids({ source, target: to });
      } catch (e) {
        fail(e);
      }
    })();
  }, [target?.id]);

  function request() {
    if (!target || !tables) return;
    if (!needsProductionConfirm(target)) return void run();
    const production = tables.map((table) => `CREATE TABLE ${targetSchema}.${table} (...) -- ${t("and every row of {table}", { table: `${schema}.${table}` })}`);
    setConfirm({ connection: target, unfiltered: [], production, blocked: false });
  }

  async function run() {
    if (!sids || !tables || !target) return;
    await start(
      (onEvent) => api.migrateTables(sids.source, sids.target, schema, targetSchema, tables, whole, onEvent),
      () => requestNavigatorRefresh({ connectionId: target.id, database: null, schema: targetSchema }),
    );
  }

  async function build() {
    if (!target || !tables) return null;
    return {
      kind: "migrate" as const,
      source_connection_id: connection.id,
      source_database: database,
      target_connection_id: target.id,
      target_database: null,
      source_schema: schema,
      target_schema: targetSchema,
      // A whole schema is saved as such, so the tables added later are copied too.
      tables: chosen === null ? null : tables,
      objects: whole,
    };
  }

  const running = status.state === "running";
  const close = () => !running && onClose();
  const count = tables?.length ?? 0;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal migrate" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{!whole ? plural(count, "Copy {n} table to a database", "Copy {n} tables to a database") : t("Copy {schema} to a database", { schema })}</h2>
        <p className="muted small">
          {tables === null
            ? t("Reading the tables…")
            : `${t(
                !whole
                  ? "Each table is created with its columns, NOT NULL, primary key and identity, then filled; defaults, indexes, foreign keys and CHECKs come last."
                  : "Each table is created with its columns, NOT NULL, primary key and identity, then filled; defaults, indexes, foreign keys and CHECKs come last, then the views and routines.",
              )} ${tables.join(", ") || t("No tables")}.`}
        </p>
        {saved && !candidates.some((c) => c.id === saved.target_connection_id) && (
          <div className="notice warn small">{t("The target connection of the profile is gone or takes no tables: pick another.")}</div>
        )}
        {gone.length > 0 && (
          <div className="notice warn small">{t("No longer in the database, left out: {names}.", { names: gone.join(", ") })}</div>
        )}
        {targets.length === 0 && <div className="notice small">{t("Save another connection to copy the tables to.")}</div>}
        {candidates.length > 0 && (
          <div className="row">
            <label className="field grow">
              <span>{t("Connection")}</span>
              <select
                value={target?.id ?? ""}
                disabled={running}
                onChange={(e) => setTarget(targets.find((c) => c.id === e.target.value) ?? null)}
              >
                {candidates.map((c) => (
                  <option key={c.id} value={c.id} disabled={takesNoRows(c)}>
                    {c.read_only
                      ? t("{name} (read-only)", { name: c.name })
                      : takesNoRows(c)
                        ? t("{name} (rows by SQL only)", { name: c.name })
                        : c.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              <span>{t("Schema")}</span>
              <select value={targetSchema} disabled={running} onChange={(e) => setTargetSchema(e.target.value)}>
                {schemas.map((s) => (
                  <option key={s.name}>{s.name}</option>
                ))}
              </select>
            </label>
          </div>
        )}
        <JobFooter status={status} messages={[msg("Copying… {n} rows"), msg("Copied {n} rows.")]} onClose={onClose}>
          <ProfileButtons profile={profile} build={build} disabled={running || !target || !targetSchema || count === 0} />
          <button
            type="button"
            className="primary"
            disabled={running || status.state === "done" || !sids || !targetSchema || count === 0 || (!!target && takesNoRows(target))}
            onClick={request}
          >
            {t("Copy")}
          </button>
        </JobFooter>
      </div>
      {confirm && (
        <GuardDialog
          request={confirm}
          onAnswer={(go) => {
            setConfirm(null);
            if (go) void run();
          }}
        />
      )}
    </div>
  );
}
