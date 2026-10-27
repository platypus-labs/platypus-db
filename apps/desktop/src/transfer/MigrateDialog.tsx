import { useEffect, useState } from "react";
import { api, isMongo, opensDataFile, type SavedConnection, type SchemaMeta } from "../api";
import { GuardDialog } from "../guards/GuardDialog";
import { type GuardRequest, needsProductionConfirm } from "../guards/guards";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { workbench } from "../workbench";
import { JobFooter, useJob } from "./job";
import { t, plural, msg } from "../i18n";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  /** The tables to copy; `null` for every table of the schema. */
  tables: string[] | null;
  onClose: () => void;
}

/** Copies tables, structure and rows, to a schema of another database: each is created with
 *  its columns mapped, NOT NULL, primary key and identity, then the defaults, indexes, foreign
 *  keys and CHECKs follow; a whole schema brings its views and routines too. */
export function MigrateDialog({ connection, database, schema, tables: chosen, onClose }: Props) {
  const candidates = workbench()
    .connections()
    .filter((c) => c.id !== connection.id && !opensDataFile(c) && !isMongo(c.driver));
  // Read-only connections are listed, but take no tables.
  const targets = candidates.filter((c) => !c.read_only);
  const [tables, setTables] = useState<string[] | null>(chosen);
  const [target, setTarget] = useState<SavedConnection | null>(targets[0] ?? null);
  const [schemas, setSchemas] = useState<SchemaMeta[]>([]);
  const [targetSchema, setTargetSchema] = useState("");
  const [sids, setSids] = useState<{ source: number; target: number } | null>(null);
  const { status, start, fail } = useJob();
  const [confirm, setConfirm] = useState<GuardRequest | null>(null);

  useEffect(() => {
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
        setTargetSchema((all.find((s) => s.current) ?? all[0])?.name ?? "");
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
      (onEvent) => api.migrateTables(sids.source, sids.target, schema, targetSchema, tables, chosen === null, onEvent),
      () => requestNavigatorRefresh({ connectionId: target.id, database: null, schema: targetSchema }),
    );
  }

  const running = status.state === "running";
  const close = () => !running && onClose();
  const count = tables?.length ?? 0;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal migrate" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{chosen ? plural(count, "Copy {n} table to a database", "Copy {n} tables to a database") : t("Copy {schema} to a database", { schema })}</h2>
        <p className="muted small">
          {tables === null
            ? t("Reading the tables…")
            : `${t(
                chosen
                  ? "Each table is created with its columns, NOT NULL, primary key and identity, then filled; defaults, indexes, foreign keys and CHECKs come last."
                  : "Each table is created with its columns, NOT NULL, primary key and identity, then filled; defaults, indexes, foreign keys and CHECKs come last, then the views and routines.",
              )} ${tables.join(", ") || t("No tables")}.`}
        </p>
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
                  <option key={c.id} value={c.id} disabled={c.read_only}>
                    {c.read_only ? t("{name} (read-only)", { name: c.name }) : c.name}
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
          <button
            type="button"
            className="primary"
            disabled={running || status.state === "done" || !sids || !targetSchema || count === 0 || !!target?.read_only}
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
