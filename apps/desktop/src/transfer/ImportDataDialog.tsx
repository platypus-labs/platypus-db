import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  type ColumnMeta,
  DATA_FILE_EXTENSIONS,
  type ExportEvent,
  type ImportMode,
  type ImportPlan,
  type ImportPreview,
  opensDataFile,
  type SavedConnection,
  type SchemaMeta,
  toCommandError,
} from "../api";
import { GuardDialog } from "../guards/GuardDialog";
import { type GuardRequest, needsProductionConfirm } from "../guards/guards";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { workbench } from "../workbench";
import { t, plural, msg } from "../i18n";

/** A table of a connection: where rows come from or go to. */
export interface TableAt {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  /** Whether `schema` is the session's current one, whose tables are named unqualified. */
  currentSchema: boolean;
  /** `null` for a new table (as a target). */
  table: string | null;
}

interface Props {
  /** Where the rows go. For a transfer it is where the dialog starts, and can change. */
  target: TableAt;
  /** A table to copy, instead of a file: the dialog is a transfer between databases. */
  source?: TableAt & { table: string };
  /** A file to start from, e.g. dropped on the window. */
  path?: string;
  /** The name a new table starts with. */
  newTable?: string;
  onClose: () => void;
}

/** A source column of a new table: whether it is copied, and its name and type there. */
interface NewColumn {
  include: boolean;
  target: string;
  type: string;
}

type Status =
  | { state: "idle" }
  | { state: "running"; id: number | null; rows: number }
  | { state: "done"; rows: number; ms: number }
  | { state: "failed"; message: string };

const MODES: { value: ImportMode; label: string }[] = [
  { value: "append", label: msg("Add the rows") },
  { value: "replace", label: msg("Replace all rows") },
  { value: "upsert", label: msg("Update rows with the same key, add the others") },
];

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
const failure = (e: unknown) => toCommandError(e).message;

/** Copies rows into a table: from a CSV, TSV, JSON, NDJSON, Parquet or Excel file, or from a
 *  table of another database. The target is an existing table, each of its columns fed from
 *  a source column, or a new one created with the source types mapped to the target's. */
export function ImportDataDialog({ target: start, source, path: initial, newTable: name, onClose }: Props) {
  const transfer = source !== undefined;
  const [target, setTarget] = useState(start);
  const [sid, setSid] = useState<number | null>(null);
  const [sourceSid, setSourceSid] = useState<number | null>(null);
  /** The target connection's schemas and the tables of the target schema, for a transfer. */
  const [schemas, setSchemas] = useState<SchemaMeta[]>([]);
  const [tables, setTables] = useState<string[]>([]);
  const [path, setPath] = useState(initial ?? "");
  /** The file the preview shows: an edited path is read again on Enter. */
  const [loaded, setLoaded] = useState<string | null>(null);
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [newTable, setNewTable] = useState(name ?? source?.table ?? "");
  const [newColumns, setNewColumns] = useState<NewColumn[]>([]);
  const [targets, setTargets] = useState<ColumnMeta[]>([]);
  /** For each target column, the source column that feeds it ("" for none). */
  const [mapping, setMapping] = useState<string[]>([]);
  const [mode, setMode] = useState<ImportMode>("append");
  const [status, setStatus] = useState<Status>({ state: "idle" });
  const [error, setError] = useState<string | null>(null);
  const fail = (e: unknown) => setError(failure(e));
  /** The confirmation a production connection asks before the import. */
  const [confirm, setConfirm] = useState<GuardRequest | null>(null);
  /** Bumped by each preview request: only the latest one is shown. */
  const previewToken = useRef(0);

  /** The existing table the rows go to; `null` creates `newTable`. */
  const existing = transfer ? (tables.find((t) => same(t, newTable.trim())) ?? null) : target.table;

  useEffect(() => {
    if (source) {
      workbench()
        .sideSession(source.connection, source.database)
        .then(setSourceSid, fail);
    }
  }, []);

  // Each answer is dropped once the target changed again: a slower answer for an earlier
  // connection, schema or table must not replace the one now chosen.
  useEffect(() => {
    setSid(null);
    setSchemas([]);
    setTables([]);
    let live = true;
    workbench()
      .sideSession(target.connection, target.database)
      .then(async (id) => {
        if (!live) return;
        setSid(id);
        if (id === null || !transfer) return;
        const found = await api.listSchemas(id);
        if (live) setSchemas(found);
      })
      .catch((e: unknown) => live && fail(e));
    return () => {
      live = false;
    };
  }, [target.connection.id, target.database]);

  useEffect(() => {
    setTables([]);
    if (sid === null || !transfer || !target.schema) return;
    let live = true;
    api.listObjects(sid, target.schema).then(
      (objects) => live && setTables(objects.filter((o) => o.kind === "table").map((o) => o.name)),
      (e: unknown) => live && fail(e),
    );
    return () => {
      live = false;
    };
  }, [sid, target.schema]);

  useEffect(() => {
    setTargets([]);
    if (sid === null || existing === null || !target.schema) return;
    let live = true;
    api.tableDetails(sid, target.schema, existing).then(
      (d) => live && setTargets(d.columns),
      (e: unknown) => live && fail(e),
    );
    return () => {
      live = false;
    };
  }, [sid, target.schema, existing]);

  // The preview: a file once chosen, a source table as soon as both sessions are open (its
  // types depend on the target's database).
  useEffect(() => {
    if (sid === null) return;
    if (source && sourceSid !== null) void loadTable(sourceSid, sid);
    else if (!source && path) void loadFile(path, null);
  }, [sid, sourceSid]);

  // Each target column takes the source column of the same name, ignoring case.
  useEffect(() => {
    if (!preview) return;
    const names = preview.columns.map((c) => c.name);
    setMapping(targets.map((t) => names.find((n) => same(n, t.name)) ?? ""));
  }, [preview, targets]);

  function show(p: ImportPreview) {
    setPreview(p);
    setNewColumns(p.columns.map((c) => ({ include: true, target: c.name, type: c.target_type })));
  }

  async function loadTable(from: number, to: number) {
    const token = ++previewToken.current;
    setError(null);
    try {
      const p = await api.previewTransfer(from, source!.currentSchema ? null : source!.schema, source!.table, to);
      if (token === previewToken.current) show(p);
    } catch (e) {
      if (token !== previewToken.current) return;
      setPreview(null);
      setError(failure(e));
    }
  }

  async function loadFile(file: string, sheet: string | null) {
    if (sid === null) return;
    const token = ++previewToken.current;
    setError(null);
    setStatus({ state: "idle" });
    try {
      const p = await api.previewImport(sid, file, sheet);
      if (token !== previewToken.current) return;
      show(p);
      setLoaded(file);
      setNewTable(p.table);
    } catch (e) {
      if (token !== previewToken.current) return;
      setPreview(null);
      setLoaded(null);
      setError(failure(e));
    }
  }

  async function choose() {
    const file = await open({
      multiple: false,
      title: t("Import data from a file"),
      filters: [{ name: t("Data files"), extensions: DATA_FILE_EXTENSIONS }],
    });
    if (typeof file !== "string") return;
    setPath(file);
    await loadFile(file, null);
  }

  function plan(p: ImportPreview): ImportPlan {
    const base = {
      source_table: p.table,
      source_schema: source && !source.currentSchema ? source.schema : null,
      schema: target.currentSchema ? null : target.schema,
    };
    if (existing === null) {
      return {
        ...base,
        table: newTable.trim(),
        create: true,
        columns: p.columns.flatMap((c, i) => {
          const n = newColumns[i];
          return n?.include ? [{ source: c.name, target: n.target.trim(), data_type: n.type.trim() }] : [];
        }),
      };
    }
    return {
      ...base,
      table: existing,
      create: false,
      mode,
      keys: mode === "upsert" ? keys : [],
      columns: targets.flatMap((t, i) => (mapping[i] ? [{ source: mapping[i], target: t.name, data_type: null }] : [])),
    };
  }

  /** Runs the import, once confirmed on a production connection. */
  function request() {
    if (!preview) return;
    if (!needsProductionConfirm(target.connection)) return void run();
    setConfirm({ connection: target.connection, unfiltered: [], production: changes(plan(preview)), blocked: false });
  }

  function onEvent(event: ExportEvent) {
    switch (event.type) {
      case "progress":
        setStatus((s) => (s.state === "running" ? { ...s, rows: event.rows } : s));
        break;
      case "done":
        setStatus({ state: "done", rows: event.rows, ms: event.elapsed_ms });
        requestNavigatorRefresh({ connectionId: target.connection.id, database: target.database, schema: target.schema });
        break;
      case "failed":
        setStatus({ state: "failed", message: event.error.message });
        break;
      case "cancelled":
        setStatus({ state: "idle" });
    }
  }

  async function run() {
    if (sid === null || !preview) return;
    setStatus({ state: "running", id: null, rows: 0 });
    try {
      const p = plan(preview);
      const id = source ? await api.transferTable(sourceSid!, sid, p, onEvent) : await api.importFile(sid, loaded!, p, onEvent);
      setStatus((s) => (s.state === "running" ? { ...s, id } : s));
    } catch (e) {
      setStatus({ state: "failed", message: failure(e) });
    }
  }

  /** Points the transfer at another connection, in its current schema. */
  function chooseConnection(id: string) {
    const connection = workbench().connections().find((c) => c.id === id);
    if (!connection) return;
    setSchemas([]);
    setTables([]);
    setTarget({ connection, database: null, schema: "", currentSchema: true, table: null });
  }

  // A new target connection starts in its current schema.
  useEffect(() => {
    if (!transfer || target.schema || schemas.length === 0) return;
    const current = schemas.find((s) => s.current) ?? schemas[0]!;
    setTarget((t) => ({ ...t, schema: current.name, currentSchema: current.current }));
  }, [schemas]);

  const running = status.state === "running";
  /** The primary key of the existing table: what an upsert matches. */
  const keys = targets.filter((t) => t.primary_key).map((t) => t.name);
  const p = preview ? plan(preview) : null;
  const ready =
    p !== null &&
    (transfer ? sourceSid !== null : loaded === path.trim()) &&
    p.table !== "" &&
    p.columns.length > 0 &&
    (p.mode !== "upsert" || keys.every((k) => p.columns.some((c) => c.target === k))) &&
    p.columns.every((c) => c.target !== "" && (c.data_type === null || c.data_type !== ""));
  const close = () => !running && onClose();
  const editColumn = (i: number, change: Partial<NewColumn>) =>
    setNewColumns((cs) => cs.map((c, j) => (j === i ? { ...c, ...change } : c)));
  const from = t(transfer ? "Source column" : "File column");
  const title = source
    ? t("Copy {table} to a database", { table: source.table })
    : target.table === null
      ? t("Import data into a new table in {schema}", { schema: target.schema })
      : t("Import data into {table}", { table: target.table });

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal wide import-data" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{title}</h2>
        {transfer ? (
          <div className="row">
            <label className="field grow">
              <span>{t("Connection")}</span>
              <select value={target.connection.id} disabled={running} onChange={(e) => chooseConnection(e.target.value)}>
                {workbench()
                  .connections()
                  // Read-only connections take no data.
                  .filter((c) => c.id === target.connection.id || (!opensDataFile(c) && !c.read_only))
                  .map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name}
                    </option>
                  ))}
              </select>
            </label>
            <label className="field">
              <span>{t("Schema")}</span>
              <select
                value={target.schema}
                disabled={running}
                onChange={(e) => {
                  const s = schemas.find((x) => x.name === e.target.value)!;
                  setTarget({ ...target, schema: s.name, currentSchema: s.current });
                }}
              >
                {schemas.map((s) => (
                  <option key={s.name}>{s.name}</option>
                ))}
              </select>
            </label>
            <label className="field grow">
              <span>{t(existing === null ? "New table" : "Existing table")}</span>
              <input value={newTable} disabled={running} onChange={(e) => setNewTable(e.target.value)} list="import-tables" />
              <datalist id="import-tables">
                {tables.map((table) => (
                  <option key={table} value={table} />
                ))}
              </datalist>
            </label>
          </div>
        ) : (
          <div className="row">
            <label className="field grow">
              <span>{t("File")}</span>
              <input
                value={path}
                disabled={running}
                placeholder={t("CSV, TSV, JSON, NDJSON, Parquet or Excel")}
                title={path}
                onChange={(e) => setPath(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && path.trim() && void loadFile(path.trim(), null)}
              />
            </label>
            <button type="button" className="align-end" disabled={running || sid === null} onClick={() => void choose()}>
              {t("Choose file…")}
            </button>
            {preview && preview.tables.length > 1 && (
              <label className="field">
                <span>{t("Sheet")}</span>
                <select value={preview.table} disabled={running} onChange={(e) => void loadFile(loaded!, e.target.value)}>
                  {preview.tables.map((sheet) => (
                    <option key={sheet}>{sheet}</option>
                  ))}
                </select>
              </label>
            )}
          </div>
        )}
        {error && <div className="notice error small">{error}</div>}
        {target.connection.read_only && (
          <div className="notice warn small">{t("{name} is read-only: nothing is written there.", { name: target.connection.name })}</div>
        )}
        {preview && (
          <>
            <div className="import-preview">
              <table>
                <thead>
                  <tr>
                    {preview.columns.map((c) => (
                      <th key={c.name}>
                        {c.name} <span className="muted small">{c.source_type}</span>
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {preview.rows.map((row, r) => (
                    <tr key={r}>
                      {row.map((v, c) => (
                        <td key={c} className={v === null ? "muted" : undefined}>
                          {v ?? "NULL"}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="muted small">
              {plural(preview.rows.length, "First {n} row of {table}.", "First {n} rows of {table}.", { table: preview.table })}
            </p>
            {existing === null ? (
              <>
                {!transfer && (
                  <label className="field">
                    <span>{t("Table name")}</span>
                    <input value={newTable} disabled={running} onChange={(e) => setNewTable(e.target.value)} />
                  </label>
                )}
                <div className="import-mapping">
                  <span className="muted small">{from}</span>
                  <span className="muted small">{t("Column")}</span>
                  <span className="muted small">{t("Type")}</span>
                  {preview.columns.map((c, i) => (
                    <NewColumnRow key={c.name} name={c.name} column={newColumns[i]} disabled={running} onChange={(ch) => editColumn(i, ch)} />
                  ))}
                </div>
              </>
            ) : (
              <>
                <div className="import-modes" role="radiogroup" aria-label={t("Rows already in the table")}>
                  {MODES.map((m) => {
                    const off = m.value === "upsert" && keys.length === 0;
                    return (
                      <label key={m.value} className="check" title={off ? t("The table has no primary key") : undefined}>
                        <input
                          type="radio"
                          name="import-mode"
                          checked={mode === m.value}
                          disabled={running || off}
                          onChange={() => setMode(m.value)}
                        />
                        {t(m.label)}
                      </label>
                    );
                  })}
                </div>
                <div className="import-mapping two">
                  <span className="muted small">{t("Column of {table}", { table: existing })}</span>
                  <span className="muted small">{t("From {source}", { source: from.toLowerCase() })}</span>
                  {targets.map((column, i) => (
                    <MappingRow
                      key={column.name}
                      target={column}
                      sources={preview.columns.map((c) => c.name)}
                      value={mapping[i] ?? ""}
                      disabled={running}
                      onChange={(v) => setMapping((m) => m.map((x, j) => (j === i ? v : x)))}
                    />
                  ))}
                </div>
              </>
            )}
          </>
        )}
        {status.state === "running" && <div className="notice small">{plural(status.rows, "Copying… {n} row", "Copying… {n} rows")}</div>}
        {status.state === "done" && (
          <div className="notice small">
            {plural(status.rows, "Imported {n} row in {time} s.", "Imported {n} rows in {time} s.", { time: (status.ms / 1000).toFixed(1) })}
          </div>
        )}
        {status.state === "failed" && <div className="notice error small">{t("Nothing was imported: {message}", { message: status.message })}</div>}
        <div className="actions">
          <span className="spacer" />
          {running ? (
            <button type="button" disabled={status.id === null} onClick={() => status.id !== null && void api.cancelExport(status.id)}>
              {t("Cancel import")}
            </button>
          ) : (
            <button type="button" onClick={onClose}>
              {t(status.state === "done" ? "Close" : "Cancel")}
            </button>
          )}
          <button type="button" className="primary" disabled={!ready || running || status.state === "done" || target.connection.read_only} onClick={request}>
            {t(transfer ? "Copy" : "Import")}
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

/** What the import changes, as statements, for the production confirmation. */
function changes(p: ImportPlan): string[] {
  const table = p.schema ? `${p.schema}.${p.table}` : p.table;
  const columns = p.columns.map((c) => c.target).join(", ");
  const rows = `VALUES ... -- ${t("every row of {table}", { table: p.source_table })}`;
  return [
    ...(p.mode === "replace" ? [`DELETE FROM ${table}`] : []),
    ...(p.create ? [`CREATE TABLE ${table} (${p.columns.map((c) => `${c.target} ${c.data_type}`).join(", ")})`] : []),
    p.mode === "upsert"
      ? `INSERT INTO ${table} (${columns}) ${rows}, ${t("updating the rows with the same {keys}", { keys: p.keys!.join(", ") })}`
      : `INSERT INTO ${table} (${columns}) ${rows}`,
  ];
}

function NewColumnRow(props: { name: string; column: NewColumn | undefined; disabled: boolean; onChange: (c: Partial<NewColumn>) => void }) {
  const { name, column, disabled, onChange } = props;
  if (!column) return null;
  const off = disabled || !column.include;
  return (
    <>
      <label className="check">
        <input type="checkbox" checked={column.include} disabled={disabled} onChange={(e) => onChange({ include: e.target.checked })} />
        {name}
      </label>
      <input value={column.target} disabled={off} aria-label={t("Name of {column}", { column: name })} onChange={(e) => onChange({ target: e.target.value })} />
      <input value={column.type} disabled={off} aria-label={t("Type of {column}", { column: name })} onChange={(e) => onChange({ type: e.target.value })} />
    </>
  );
}

function MappingRow(props: { target: ColumnMeta; sources: string[]; value: string; disabled: boolean; onChange: (v: string) => void }) {
  const { target, sources, value, disabled, onChange } = props;
  return (
    <>
      <span>
        {target.name} <span className="muted small">{target.data_type}</span>
      </span>
      <select value={value} disabled={disabled} aria-label={t("Source of {column}", { column: target.name })} onChange={(e) => onChange(e.target.value)}>
        <option value="">{t("— not imported —")}</option>
        {sources.map((f) => (
          <option key={f}>{f}</option>
        ))}
      </select>
    </>
  );
}
