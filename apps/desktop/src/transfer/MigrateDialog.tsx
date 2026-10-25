import { useEffect, useState } from "react";
import { api, isDataFile, type SavedConnection, type SchemaMeta, toCommandError } from "../api";
import { GuardDialog } from "../guards/GuardDialog";
import { type GuardRequest, needsProductionConfirm } from "../guards/guards";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { workbench } from "../workbench";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  /** The tables to copy; `null` for every table of the schema. */
  tables: string[] | null;
  onClose: () => void;
}

type Status =
  | { state: "idle" }
  | { state: "running"; id: number | null; rows: number }
  | { state: "done"; rows: number; warnings: string[] }
  | { state: "failed"; message: string };

const failure = (e: unknown) => toCommandError(e).message;

/** Copies tables, structure and rows, to a schema of another database: each is created with
 *  its columns mapped, NOT NULL and primary key, then the foreign keys between them follow. */
export function MigrateDialog({ connection, database, schema, tables: chosen, onClose }: Props) {
  // A data file's database lives in memory: a copy there would be lost.
  const targets = workbench()
    .connections()
    .filter((c) => c.id !== connection.id && !(c.driver === "duckdb" && isDataFile(c.database)));
  const [tables, setTables] = useState<string[] | null>(chosen);
  const [target, setTarget] = useState<SavedConnection | null>(targets[0] ?? null);
  const [schemas, setSchemas] = useState<SchemaMeta[]>([]);
  const [targetSchema, setTargetSchema] = useState("");
  const [sids, setSids] = useState<{ source: number; target: number } | null>(null);
  const [status, setStatus] = useState<Status>({ state: "idle" });
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
      .catch((e: unknown) => setStatus({ state: "failed", message: failure(e) }));
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
        setStatus({ state: "failed", message: failure(e) });
      }
    })();
  }, [target?.id]);

  function request() {
    if (!target || !tables) return;
    if (!needsProductionConfirm(target)) return void run();
    const production = tables.map((t) => `CREATE TABLE ${targetSchema}.${t} (...) -- and every row of ${schema}.${t}`);
    setConfirm({ connection: target, unfiltered: [], production, blocked: false });
  }

  async function run() {
    if (!sids || !tables || !target) return;
    setStatus({ state: "running", id: null, rows: 0 });
    try {
      const id = await api.migrateTables(sids.source, sids.target, schema, targetSchema, tables, (event) => {
        if (event.type === "progress") setStatus((s) => (s.state === "running" ? { ...s, rows: event.rows } : s));
        else if (event.type === "failed") setStatus({ state: "failed", message: event.error.message });
        else if (event.type === "cancelled") setStatus({ state: "idle" });
        else {
          setStatus({ state: "done", rows: event.rows, warnings: event.warnings ?? [] });
          requestNavigatorRefresh({ connectionId: target.id, database: null, schema: targetSchema });
        }
      });
      setStatus((s) => (s.state === "running" ? { ...s, id } : s));
    } catch (e) {
      setStatus({ state: "failed", message: failure(e) });
    }
  }

  const running = status.state === "running";
  const close = () => !running && onClose();
  const count = tables?.length ?? 0;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal migrate" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{chosen ? `Copy ${count} ${count === 1 ? "table" : "tables"} to a database` : `Copy ${schema} to a database`}</h2>
        <p className="muted small">
          {tables === null
            ? "Reading the tables…"
            : `Each table is created with its columns, NOT NULL and primary key, then filled; the foreign keys between them come last. ${tables.join(", ") || "No tables"}.`}
        </p>
        {targets.length === 0 ? (
          <div className="notice small">Save another connection to copy the tables to.</div>
        ) : (
          <div className="row">
            <label className="field grow">
              <span>Connection</span>
              <select
                value={target?.id ?? ""}
                disabled={running}
                onChange={(e) => setTarget(targets.find((c) => c.id === e.target.value) ?? null)}
              >
                {targets.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              <span>Schema</span>
              <select value={targetSchema} disabled={running} onChange={(e) => setTargetSchema(e.target.value)}>
                {schemas.map((s) => (
                  <option key={s.name}>{s.name}</option>
                ))}
              </select>
            </label>
          </div>
        )}
        {status.state === "running" && <div className="notice small">Copying… {status.rows.toLocaleString()} rows</div>}
        {status.state === "done" && (
          <div className={`notice small${status.warnings.length ? " warn" : ""}`}>
            Copied {status.rows.toLocaleString()} rows.
            {status.warnings.length > 0 && (
              <>
                {" "}
                Left out:
                <ul className="migrate-warnings">
                  {status.warnings.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              </>
            )}
          </div>
        )}
        {status.state === "failed" && <div className="notice error small">{status.message}</div>}
        <div className="actions">
          <span className="spacer" />
          {running ? (
            <button type="button" disabled={status.id === null} onClick={() => status.id !== null && void api.cancelExport(status.id)}>
              Stop
            </button>
          ) : (
            <button type="button" onClick={onClose}>
              {status.state === "done" ? "Close" : "Cancel"}
            </button>
          )}
          <button
            type="button"
            className="primary"
            disabled={running || status.state === "done" || !sids || !targetSchema || count === 0}
            onClick={request}
          >
            Copy
          </button>
        </div>
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
