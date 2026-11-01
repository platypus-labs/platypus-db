/** "Query across databases": tables of different connections (a PostgreSQL table and a MySQL
 *  one) are copied into one in-memory DuckDB, and an ordinary SQL editor opens on it, where a
 *  query joins them. Like "Query with SQL" the database lives behind a temporary connection;
 *  "Reload sources" copies the tables again. */
import { createElement, useEffect, useRef, useState } from "react";
import { registerAction } from "../actions";
import { type AcrossSource, api, isNoSql, type ObjectKind, type SavedConnection, type SchemaMeta, toCommandError } from "../api";
import { msg, plural, t } from "../i18n";
import { DRIVER_NAMES } from "../drivers/names";
import { withDriver } from "../drivers/DriverPopup";
import { workbench } from "../workbench";
import { noteZones, openTemporaryQuery, ReloadBar } from "./QueryWithSql";
import {
  type AcrossLoaded,
  type AcrossOrigin,
  type AcrossSourceRef,
  assignNames,
  connectionTitle,
  initialSql,
  type NameProblem,
  nameProblems,
} from "./acrossModel";
import { Icon } from "../ui/Icon";
import { Busy } from "../ui/State";

/** The kinds of object whose rows can be copied. */
const READABLE: ObjectKind[] = ["table", "view", "materialized_view", "foreign_table"];

/** A table the dialog was opened with. */
export interface AcrossStart {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  table: string;
}

/** One source in the dialog. */
interface Row {
  key: number;
  connectionId: string;
  database: string | null;
  schema: string;
  table: string;
  filter: string;
  /** The name was typed: it stays as typed. */
  edited: boolean;
  name: string;
}

const sqlConnections = () => workbench().connections().filter((c) => !isNoSql(c.driver));

/** The session of each source and how errors name it; `null` when a password prompt was
 *  cancelled or a connection is gone. */
async function acrossSources(refs: AcrossSourceRef[]): Promise<AcrossSource[] | null> {
  const request: AcrossSource[] = [];
  for (const r of refs) {
    const connection = workbench().connections().find((c) => c.id === r.connectionId);
    if (!connection) {
      workbench().showError(t("The connection {name} no longer exists.", { name: r.connectionName }));
      return null;
    }
    const session = await workbench().sideSession(connection, r.database);
    if (session === null) return null;
    request.push({
      session_id: session,
      schema: r.schema === "" ? null : r.schema,
      table: r.table,
      filter: r.filter.trim() === "" ? null : r.filter.trim(),
      name: r.name.trim(),
      label: `${r.connectionName} · ${r.schema === "" ? r.table : `${r.schema}.${r.table}`}`,
    });
  }
  return request;
}

/** Opens the dialog; `start` are the tables chosen at first (else one empty source). */
function queryAcross(start: AcrossStart[] = []) {
  workbench().openDialog((close) => createElement(CrossQueryDialog, { start, onClose: close }));
}

let nextKey = 1;

function CrossQueryDialog({ start, onClose }: { start: AcrossStart[]; onClose: () => void }) {
  const connections = sqlConnections();
  const [rows, setRows] = useState<Row[]>(() =>
    (start.length > 0 ? start : connections.slice(0, 1).map((connection) => ({ connection, database: null, schema: "", table: "" })))
      .map((s) => ({
        key: nextKey++,
        connectionId: s.connection.id,
        database: s.database,
        schema: s.schema,
        table: s.table,
        filter: "",
        edited: false,
        name: "",
      })),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const names = assignNames(rows.map((r) => ({ connectionName: nameOf(connections, r.connectionId), table: r.table, edited: r.edited, name: r.name })));
  const problems = nameProblems(names);
  const ready = rows.length > 0 && rows.every((r) => r.table !== "") && problems.every((p) => p === null);

  const change = (key: number, patch: Partial<Row>) => setRows((all) => all.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  const add = () => {
    const last = rows.at(-1);
    const connection = connections.find((c) => c.id === last?.connectionId) ?? connections[0];
    if (!connection) return;
    setRows([...rows, { key: nextKey++, connectionId: connection.id, database: last?.database ?? null, schema: "", table: "", filter: "", edited: false, name: "" }]);
  };

  async function open() {
    setBusy(true);
    setError(null);
    try {
      const refs: AcrossSourceRef[] = rows.map((r, i) => ({
        connectionId: r.connectionId,
        connectionName: nameOf(connections, r.connectionId),
        database: r.database,
        schema: r.schema,
        table: r.table,
        filter: r.filter,
        name: (names[i] ?? "").trim(),
      }));
      const request = await acrossSources(refs);
      if (request === null) return;
      const title = connectionTitle(refs.map((r) => r.name));
      const opened = await withDriver(() => api.openAcrossDb(title, request));
      onClose();
      openTemporaryQuery(opened.connection, { sql: initialSql(opened.tables), title: t("Across databases"), across: originOf(refs, opened.tables) });
      noteZones(opened.zones);
    } catch (e) {
      // The message starts with the source that failed.
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="modal across" role="dialog" aria-modal="true" aria-label={t("Query across databases")} onKeyDown={(e) => e.key === "Escape" && !busy && onClose()}>
        <h2>{t("Query across databases")}</h2>
        <p className="muted small">
          {t("Copies the tables into an in-memory DuckDB, up to the memory limit, and opens a SQL editor on it: a query there can join tables of different databases. The tables are copies; the databases are not changed.")}
        </p>
        <div className="across-list">
          {rows.map((r, i) => (
            <SourceRow
              key={r.key}
              row={r}
              connections={connections}
              name={names[i] ?? ""}
              problem={problems[i] ?? null}
              disabled={busy}
              onChange={(patch) => change(r.key, patch)}
              onRemove={rows.length > 1 ? () => setRows(rows.filter((x) => x.key !== r.key)) : undefined}
            />
          ))}
        </div>
        <div className="row tight">
          <button type="button" data-action="add-source" onClick={add} disabled={busy || connections.length === 0}>
            {t("Add table")}
          </button>
        </div>
        {error && <p className="notice error">{error}</p>}
        <div className="actions">
          <span className="muted small">{plural(rows.length, "{n} table", "{n} tables")}</span>
          <span className="spacer" />
          <button type="button" data-action="cancel" disabled={busy} onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="button" className="primary" data-action="open" disabled={busy || !ready} onClick={() => void open()}>
            {busy ? <Busy label={t("Loading…")} /> : t("Open")}
          </button>
        </div>
      </div>
    </div>
  );
}

const nameOf = (connections: SavedConnection[], id: string) => connections.find((c) => c.id === id)?.name ?? "";

const originOf = (sources: AcrossSourceRef[], tables: AcrossLoaded[]): AcrossOrigin => ({
  sources,
  cut: tables.filter((x) => x.cut).map((x) => x.name),
});

interface RowProps {
  row: Row;
  connections: SavedConnection[];
  name: string;
  problem: NameProblem | null;
  disabled: boolean;
  onChange: (patch: Partial<Row>) => void;
  onRemove?: () => void;
}

/** A source: connection, schema, table, an optional filter and the name of its copy. */
function SourceRow({ row, connections, name, problem, disabled, onChange, onRemove }: RowProps) {
  const connection = connections.find((c) => c.id === row.connectionId);
  const [schemas, setSchemas] = useState<SchemaMeta[]>([]);
  const [tables, setTables] = useState<string[]>([]);
  const [problemText, setProblemText] = useState<string | null>(null);
  // `onChange` is new at each render; the effects read the latest.
  const latest = useRef(onChange);
  latest.current = onChange;

  useEffect(() => {
    // The schemas of a connection left before they came must not replace the new one's.
    let current = true;
    setSchemas([]);
    setTables([]);
    setProblemText(null);
    if (!connection) return;
    workbench()
      .sideSession(connection, row.database)
      .then(async (sid) => {
        if (sid === null || !current) return;
        const all = (await api.listSchemas(sid)).filter((s) => !s.system || s.name === row.schema);
        if (!current) return;
        setSchemas(all);
        if (!all.some((s) => s.name === row.schema)) {
          latest.current({ schema: (all.find((s) => s.current) ?? all[0])?.name ?? "", table: "" });
        }
      })
      .catch((e) => current && setProblemText(toCommandError(e).message));
    return () => {
      current = false;
    };
  }, [row.connectionId]);

  useEffect(() => {
    let current = true;
    setTables([]);
    if (!connection || row.schema === "" || !schemas.some((s) => s.name === row.schema)) return;
    workbench()
      .sideSession(connection, row.database)
      .then(async (sid) => {
        if (sid === null || !current) return;
        const found = (await api.listObjects(sid, row.schema)).filter((o) => READABLE.includes(o.kind)).map((o) => o.name);
        if (!current) return;
        setTables(found);
        if (!found.includes(row.table)) latest.current({ table: "" });
      })
      .catch((e) => current && setProblemText(toCommandError(e).message));
    return () => {
      current = false;
    };
  }, [row.schema, schemas]);

  return (
    <div className="across-row" data-name={name}>
      <div className="row">
        <label className="field grow">
          <span>{t("Connection")}</span>
          <select
            aria-label={t("Connection")}
            value={row.connectionId}
            disabled={disabled}
            onChange={(e) => onChange({ connectionId: e.target.value, database: null, schema: "", table: "" })}
          >
            {connections.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name} ({DRIVER_NAMES[c.driver]})
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{t("Schema")}</span>
          <select aria-label={t("Schema")} value={row.schema} disabled={disabled} onChange={(e) => onChange({ schema: e.target.value, table: "" })}>
            {schemas.map((s) => (
              <option key={s.name}>{s.name}</option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{t("Table")}</span>
          <select aria-label={t("Table")} value={row.table} disabled={disabled} onChange={(e) => onChange({ table: e.target.value })}>
            {row.table === "" && (
              <option value="" disabled>
                {t("Choose a table")}
              </option>
            )}
            {tables.map((x) => (
              <option key={x}>{x}</option>
            ))}
          </select>
        </label>
        {onRemove && (
          <button type="button" className="across-remove" data-action="remove-source" disabled={disabled} onClick={onRemove} title={t("Remove")} aria-label={t("Remove")}>
            <Icon name="close" />
          </button>
        )}
      </div>
      <div className="row">
        <label className="field grow">
          <span>{t("Filter (WHERE)")}</span>
          <input
            aria-label={t("Filter (WHERE)")}
            value={row.filter}
            disabled={disabled}
            placeholder={t("every row")}
            spellCheck={false}
            onChange={(e) => onChange({ filter: e.target.value })}
          />
        </label>
        <label className="field">
          <span>{t("Name in DuckDB")}</span>
          <input
            aria-label={t("Name in DuckDB")}
            className={problem ? "invalid" : undefined}
            value={name}
            disabled={disabled}
            spellCheck={false}
            onChange={(e) => onChange({ edited: true, name: e.target.value })}
          />
        </label>
      </div>
      {problem && (
        <p className="notice error across-problem" role="alert">
          {problem === "empty" ? t("The table needs a name.") : t("Another table has this name.")}
        </p>
      )}
      {problemText && <p className="notice error">{problemText}</p>}
    </div>
  );
}

/** Copies the sources again into the tables of `connection`; `null` when cancelled. */
async function reload(origin: AcrossOrigin, connection: SavedConnection): Promise<AcrossOrigin | null> {
  const request = await acrossSources(origin.sources);
  if (request === null) return null;
  const tables = await api.reloadAcrossDb(connection.id, request);
  return originOf(origin.sources, tables);
}

/** Above the results of an "Across databases" tab: its sources, a warning when the memory
 *  limit cut one, and Reload sources. */
export function AcrossBar({
  origin,
  connection,
  onReloaded,
}: {
  origin: AcrossOrigin;
  connection: SavedConnection;
  onReloaded: (origin: AcrossOrigin) => void;
}) {
  const list = origin.sources.map((s) => `${s.name} ← ${s.connectionName} · ${s.schema === "" ? s.table : `${s.schema}.${s.table}`}${s.filter.trim() ? ` (${s.filter.trim()})` : ""}`);
  return (
    <ReloadBar
      warn={origin.cut.length > 0}
      className="across-bar"
      label={t("Reload sources")}
      action="reload-sources"
      reload={() => reload(origin, connection)}
      onReloaded={onReloaded}
    >
      <span>{t("Copies of tables of other databases: {tables}", { tables: list.join("; ") })}</span>
      {origin.cut.length > 0 && <strong>{t("Partial: the memory limit stopped the reading of {tables}.", { tables: origin.cut.join(", ") })}</strong>}
    </ReloadBar>
  );
}

const READABLE_KINDS = new Set<ObjectKind>(READABLE);

registerAction({
  id: "analysis.query_across",
  scope: "global",
  title: "Query across databases…",
  disabled: () => (sqlConnections().length === 0 ? msg("No SQL connection to read tables from") : null),
  run: () => queryAcross(),
});

registerAction({
  id: "analysis.query_across_tables",
  scope: "navigator.object",
  title: "Query across databases…",
  group: "open",
  submenu: "Analyze",
  order: 39,
  when: ({ connection, object }) => !isNoSql(connection.driver) && READABLE_KINDS.has(object.kind),
  run: (arg) =>
    queryAcross(
      (arg.selection ?? [arg])
        .filter((x) => !isNoSql(x.connection.driver) && READABLE_KINDS.has(x.object.kind))
        .map((x) => ({ connection: x.connection, database: x.database, schema: x.schema.name, table: x.object.name })),
    ),
});
