import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  type ColumnMeta,
  DATA_FILE_EXTENSIONS,
  type ImportPlan,
  type ImportPreview,
  type SavedConnection,
  toCommandError,
} from "../api";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { workbench } from "../workbench";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  /** Whether `schema` is the session's current one, whose tables are named unqualified. */
  currentSchema: boolean;
  /** The existing table to import into; `null` creates a new one. */
  table: string | null;
  /** A file to start from, e.g. dropped on the window. */
  path?: string;
  onClose: () => void;
}

/** A file column of a new table: whether it is imported, and its name and type there. */
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

const plural = (n: number, word: string) => `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;

/** Imports a CSV, TSV, JSON, NDJSON, Parquet or Excel file into a table: an existing one, with
 *  each of its columns fed from a file column, or a new one created from the inferred types. */
export function ImportDataDialog({ connection, database, schema, currentSchema, table, path: initial, onClose }: Props) {
  const [sid, setSid] = useState<number | null>(null);
  const [path, setPath] = useState(initial ?? "");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [newTable, setNewTable] = useState("");
  const [newColumns, setNewColumns] = useState<NewColumn[]>([]);
  const [targets, setTargets] = useState<ColumnMeta[]>([]);
  /** For each target column, the file column that feeds it ("" for none). */
  const [mapping, setMapping] = useState<string[]>([]);
  const [status, setStatus] = useState<Status>({ state: "idle" });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void workbench()
      .sideSession(connection, database)
      .then(async (id) => {
        setSid(id);
        if (id !== null && table !== null) setTargets((await api.tableDetails(id, schema, table)).columns);
      })
      .catch((e: unknown) => setError(toCommandError(e).message));
  }, []);

  useEffect(() => {
    if (sid !== null && path) void load(path, null);
  }, [sid]);

  // Each target column takes the file column of the same name, ignoring case.
  useEffect(() => {
    if (!preview) return;
    const names = preview.columns.map((c) => c.name);
    setMapping(targets.map((t) => names.find((n) => n.toLowerCase() === t.name.toLowerCase()) ?? ""));
  }, [preview, targets]);

  async function load(file: string, sheet: string | null) {
    if (sid === null) return;
    setError(null);
    setStatus({ state: "idle" });
    try {
      const p = await api.previewImport(sid, file, sheet);
      setPreview(p);
      setNewTable(p.table);
      setNewColumns(p.columns.map((c) => ({ include: true, target: c.name, type: c.target_type })));
    } catch (e) {
      setPreview(null);
      setError(toCommandError(e).message);
    }
  }

  async function choose() {
    const file = await open({
      multiple: false,
      title: "Import data from a file",
      filters: [{ name: "Data files", extensions: DATA_FILE_EXTENSIONS }],
    });
    if (typeof file !== "string") return;
    setPath(file);
    await load(file, null);
  }

  function plan(p: ImportPreview): ImportPlan {
    const base = { source_table: p.table, schema: currentSchema ? null : schema };
    if (table === null) {
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
      table,
      create: false,
      columns: targets.flatMap((t, i) => (mapping[i] ? [{ source: mapping[i], target: t.name, data_type: null }] : [])),
    };
  }

  async function run() {
    if (sid === null || !preview) return;
    setStatus({ state: "running", id: null, rows: 0 });
    try {
      const id = await api.importFile(sid, path, plan(preview), (event) => {
        switch (event.type) {
          case "progress":
            setStatus((s) => (s.state === "running" ? { ...s, rows: event.rows } : s));
            break;
          case "done":
            setStatus({ state: "done", rows: event.rows, ms: event.elapsed_ms });
            requestNavigatorRefresh({ connectionId: connection.id, database, schema });
            break;
          case "failed":
            setStatus({ state: "failed", message: event.error.message });
            break;
          case "cancelled":
            setStatus({ state: "idle" });
        }
      });
      setStatus((s) => (s.state === "running" ? { ...s, id } : s));
    } catch (e) {
      setStatus({ state: "failed", message: toCommandError(e).message });
    }
  }

  const running = status.state === "running";
  const p = preview ? plan(preview) : null;
  const ready =
    p !== null &&
    p.table !== "" &&
    p.columns.length > 0 &&
    p.columns.every((c) => c.target !== "" && (c.data_type === null || c.data_type !== ""));
  const close = () => !running && onClose();
  const editColumn = (i: number, change: Partial<NewColumn>) =>
    setNewColumns((cs) => cs.map((c, j) => (j === i ? { ...c, ...change } : c)));

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal wide import-data" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{table === null ? `Import data into a new table in ${schema}` : `Import data into ${table}`}</h2>
        <div className="row">
          <label className="field grow">
            <span>File</span>
            <input value={path} readOnly placeholder="CSV, TSV, JSON, NDJSON, Parquet or Excel" title={path} />
          </label>
          <button type="button" className="align-end" disabled={running || sid === null} onClick={() => void choose()}>
            Choose file…
          </button>
          {preview && preview.tables.length > 1 && (
            <label className="field">
              <span>Sheet</span>
              <select value={preview.table} disabled={running} onChange={(e) => void load(path, e.target.value)}>
                {preview.tables.map((t) => (
                  <option key={t}>{t}</option>
                ))}
              </select>
            </label>
          )}
        </div>
        {error && <div className="notice error small">{error}</div>}
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
              First {plural(preview.rows.length, "row")} of {preview.table}.
            </p>
            {table === null ? (
              <>
                <label className="field">
                  <span>Table name</span>
                  <input value={newTable} disabled={running} onChange={(e) => setNewTable(e.target.value)} />
                </label>
                <div className="import-mapping">
                  <span className="muted small">File column</span>
                  <span className="muted small">Column</span>
                  <span className="muted small">Type</span>
                  {preview.columns.map((c, i) => (
                    <NewColumnRow key={c.name} name={c.name} column={newColumns[i]} disabled={running} onChange={(ch) => editColumn(i, ch)} />
                  ))}
                </div>
              </>
            ) : (
              <div className="import-mapping two">
                <span className="muted small">Column of {table}</span>
                <span className="muted small">From file column</span>
                {targets.map((t, i) => (
                  <MappingRow
                    key={t.name}
                    target={t}
                    files={preview.columns.map((c) => c.name)}
                    value={mapping[i] ?? ""}
                    disabled={running}
                    onChange={(v) => setMapping((m) => m.map((x, j) => (j === i ? v : x)))}
                  />
                ))}
              </div>
            )}
          </>
        )}
        {status.state === "running" && <div className="notice small">Importing… {plural(status.rows, "row")}</div>}
        {status.state === "done" && (
          <div className="notice small">
            Imported {plural(status.rows, "row")} in {(status.ms / 1000).toFixed(1)} s.
          </div>
        )}
        {status.state === "failed" && (
          <div className="notice error small">Nothing was imported: {status.message}</div>
        )}
        <div className="actions">
          <span className="spacer" />
          {running ? (
            <button type="button" disabled={status.id === null} onClick={() => status.id !== null && void api.cancelExport(status.id)}>
              Cancel import
            </button>
          ) : (
            <button type="button" onClick={onClose}>
              {status.state === "done" ? "Close" : "Cancel"}
            </button>
          )}
          <button type="button" className="primary" disabled={!ready || running || status.state === "done"} onClick={() => void run()}>
            Import
          </button>
        </div>
      </div>
    </div>
  );
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
      <input value={column.target} disabled={off} aria-label={`Name of ${name}`} onChange={(e) => onChange({ target: e.target.value })} />
      <input value={column.type} disabled={off} aria-label={`Type of ${name}`} onChange={(e) => onChange({ type: e.target.value })} />
    </>
  );
}

function MappingRow(props: { target: ColumnMeta; files: string[]; value: string; disabled: boolean; onChange: (v: string) => void }) {
  const { target, files, value, disabled, onChange } = props;
  return (
    <>
      <span>
        {target.name} <span className="muted small">{target.data_type}</span>
      </span>
      <select value={value} disabled={disabled} aria-label={`Source of ${target.name}`} onChange={(e) => onChange(e.target.value)}>
        <option value="">— not imported —</option>
        {files.map((f) => (
          <option key={f}>{f}</option>
        ))}
      </select>
    </>
  );
}
