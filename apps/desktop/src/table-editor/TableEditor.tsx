import { useEffect, useState } from "react";
import {
  api,
  type ColumnDesign,
  type ConstraintDesign,
  type DriverKind,
  type TableDesign,
  type TableScript,
  toCommandError,
} from "../api";
import { catalogKey, renameCatalog } from "../catalog/catalog";
import { tableCatalogMoves } from "../catalog/renames";
import { requestNavigatorRefresh } from "../navigatorEvents";
import type { TabBase, TabProps } from "../tabs/registry";
import { Icon, objectIcon } from "../ui/Icon";
import { workbench } from "../workbench";
import { t, msg } from "../i18n";

export type TableEditorSection = "columns" | "indexes" | "foreign_keys" | "constraints" | "sql";

/** The graphical table editor: a new table, or an existing one altered. */
export interface TableEditorTabModel extends TabBase {
  kind: "table-editor";
  schema: string;
  /** The table edited; `null` for a new one until it is saved. */
  table: string | null;
  section?: TableEditorSection;
}

declare module "../tabs/registry" {
  interface TabKinds {
    "table-editor": TableEditorTabModel;
  }
}

const SECTION_LABELS: Record<TableEditorSection, string> = {
  columns: msg("Columns"),
  indexes: msg("Indexes"),
  foreign_keys: msg("Foreign keys"),
  constraints: msg("Constraints"),
  sql: "SQL",
};

/** Types offered while typing one; any other type the database knows works too. */
const TYPES: Partial<Record<DriverKind, string[]>> = {
  postgres: [
    "integer", "bigint", "smallint", "serial", "bigserial", "numeric(10,2)", "real", "double precision",
    "boolean", "text", "varchar(255)", "char(10)", "date", "time", "timestamp", "timestamptz", "interval",
    "uuid", "json", "jsonb", "bytea",
  ],
  mysql: [
    "int", "bigint", "smallint", "tinyint", "decimal(10,2)", "float", "double", "boolean", "varchar(255)",
    "char(10)", "text", "longtext", "date", "time", "datetime", "timestamp", "json", "blob",
  ],
  sqlite: ["INTEGER", "REAL", "NUMERIC", "TEXT", "BLOB"],
  duckdb: [
    "INTEGER", "BIGINT", "SMALLINT", "DECIMAL(10,2)", "DOUBLE", "BOOLEAN", "VARCHAR", "DATE", "TIME",
    "TIMESTAMP", "TIMESTAMPTZ", "INTERVAL", "UUID", "JSON", "BLOB",
  ],
  sqlserver: [
    "int", "bigint", "smallint", "tinyint", "bit", "decimal(10,2)", "float", "nvarchar(255)", "nvarchar(max)",
    "varchar(255)", "char(10)", "date", "time", "datetime2", "datetimeoffset", "uniqueidentifier",
    "varbinary(max)",
  ],
};

const ACTIONS = ["CASCADE", "SET NULL", "SET DEFAULT", "RESTRICT"];

const blank = (): TableDesign => ({
  name: "",
  columns: [
    { original: null, name: "id", data_type: "INTEGER", nullable: false, default: null, primary_key: true, comment: null },
  ],
  indexes: [],
  foreign_keys: [],
  constraints: [],
});

type Preview = { script: TableScript; sql: string } | { error: string };

type PartKey = "indexes" | "foreign_keys" | "constraints";

const orNull = (s: string) => (s.trim() === "" ? null : s);
const names = (text: string) =>
  text
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

export function TableEditor({ tab, connection, active, sideSession, catalog, onNeedCatalog, onChange }: TabProps<TableEditorTabModel>) {
  const [original, setOriginal] = useState<TableDesign | null>(null);
  const [design, setDesign] = useState<TableDesign | null>(tab.table === null ? blank : null);
  const [error, setError] = useState<string | null>(null);
  // The script of the design it was made for: Save waits until it matches the design shown.
  const [preview, setPreview] = useState<{ of: TableDesign; preview: Preview } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [running, setRunning] = useState(false);
  const [version, setVersion] = useState(0);
  const section = tab.section ?? "columns";
  const id = (what: string) => `table-editor-${tab.key}-${what}`;

  useEffect(() => {
    if (!catalog) onNeedCatalog(false);
  }, []);

  useEffect(() => {
    if (tab.table === null) return;
    let cancelled = false;
    void (async () => {
      setError(null);
      const sid = await sideSession();
      if (sid === null || cancelled) return;
      try {
        const loaded = await api.tableDesign(sid, tab.schema, tab.table!);
        if (cancelled) return;
        setOriginal(loaded);
        setDesign(loaded);
      } catch (e) {
        if (!cancelled) setError(toCommandError(e).message);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [tab.table, version]);

  const dirty = design !== null && JSON.stringify(design) !== JSON.stringify(original ?? blank());
  useEffect(() => {
    if (Boolean(tab.unsaved) !== dirty) onChange({ unsaved: dirty });
  }, [dirty]);

  useEffect(() => {
    if (!design) return;
    let current = true;
    const timer = setTimeout(() => {
      api.tableScript(connection.driver, tab.schema, original, design).then(
        (p) => current && setPreview({ of: design, preview: p }),
        (e: unknown) => current && setPreview({ of: design, preview: { error: toCommandError(e).message } }),
      );
    }, 250);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [design, original]);

  if (!design) {
    return (
      <div className="object-tab" style={{ display: active ? "flex" : "none" }}>
        <div className="object-body">{error ? <div className="notice error">{error}</div> : <div className="muted small">{t("Loading…")}</div>}</div>
      </div>
    );
  }

  const fresh = preview?.of === design ? preview.preview : null;
  const ready = fresh && !("error" in fresh) && fresh.script.statements.length > 0 ? fresh : null;
  const columnNames = design.columns.map((c) => c.name).filter(Boolean);
  const tables = Object.keys(catalog?.schemas[tab.schema] ?? {});

  function update(patch: Partial<TableDesign>) {
    setDesign((d) => d && { ...d, ...patch });
  }

  /** Changes column `i`; a rename follows into the indexes, keys and constraints. */
  function setColumn(i: number, patch: Partial<ColumnDesign>) {
    setDesign((d) => {
      if (!d) return d;
      const old = d.columns[i]!;
      const column = { ...old, ...patch };
      if (patch.primary_key) column.nullable = false;
      const columns = d.columns.map((c, j) => (j === i ? column : c));
      if (column.name === old.name) return { ...d, columns };
      return withColumns(d, columns, (cs) => cs.map((c) => (c === old.name ? column.name : c)));
    });
  }

  function removeColumn(i: number) {
    setDesign((d) => {
      if (!d) return d;
      const name = d.columns[i]!.name;
      return withColumns(
        d,
        d.columns.filter((_, j) => j !== i),
        (cs) => cs.filter((c) => c !== name),
      );
    });
  }

  function setPart<K extends PartKey>(key: K, i: number, patch: Partial<TableDesign[K][number]>) {
    setDesign((d) => d && { ...d, [key]: d[key].map((p, j) => (j === i ? { ...p, ...patch } : p)) });
  }

  function removePart(key: PartKey, i: number) {
    setDesign((d) => d && { ...d, [key]: d[key].filter((_, j) => j !== i) });
  }

  function addPart<K extends PartKey>(key: K, part: TableDesign[K][number]) {
    setDesign((d) => d && { ...d, [key]: [...d[key], part] });
  }

  const prefix = design.name || "table";

  async function apply() {
    if (!ready || !design) return;
    setRunning(true);
    setError(null);
    try {
      const sid = await sideSession();
      if (sid === null) return;
      await api.applyTableScript(sid, ready.script);
      if (original) {
        try {
          for (const m of tableCatalogMoves(original, design)) {
            await renameCatalog(
              connection.id,
              catalogKey(tab.database, tab.schema, m.from.object, m.from.column),
              catalogKey(tab.database, tab.schema, m.to.object, m.to.column),
            );
          }
        } catch (e) {
          workbench().showError(toCommandError(e).message);
        }
      }
      setConfirming(false);
      requestNavigatorRefresh({ connectionId: connection.id, database: tab.database, schema: tab.schema });
      onNeedCatalog(true);
      if (design.name !== tab.table) onChange({ table: design.name, title: design.name });
      else setVersion((v) => v + 1);
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="object-tab table-editor" style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <span className="object-icon" data-kind="table">
          <Icon name={objectIcon("table")} size={16} />
        </span>
        <label className="table-name">
          <input value={design.name} placeholder={t("Table name")} aria-label={t("Table name")} autoFocus={tab.table === null} onChange={(e) => update({ name: e.target.value })} />
          <span className="muted small">
            {t("in {schema}", { schema: tab.schema })}
            {tab.database ? ` · ${tab.database}` : ""}
          </span>
        </label>
        <span className="spacer" />
        <button className="square" disabled={!dirty} onClick={() => setDesign(original ?? blank())} title={t("Revert the changes")} aria-label={t("Revert")}>
          <Icon name="revert" />
        </button>
        <button
          className="primary"
          disabled={!ready || connection.read_only}
          title={connection.read_only ? t("{name} is read-only", { name: connection.name }) : undefined}
          onClick={() => setConfirming(true)}
        >
          {t(tab.table === null ? "Create…" : "Save…")}
        </button>
      </header>
      <nav className="object-sections">
        {(Object.keys(SECTION_LABELS) as TableEditorSection[]).map((s) => (
          <button key={s} className={s === section ? "active" : ""} onClick={() => onChange({ section: s })}>
            {t(SECTION_LABELS[s])}
            {s !== "sql" && <span className="count">{s === "columns" ? design.columns.length : design[s].length}</span>}
          </button>
        ))}
      </nav>
      <datalist id={id("types")}>
        {(TYPES[connection.driver] ?? []).map((type) => (
          <option key={type} value={type} />
        ))}
      </datalist>
      <datalist id={id("tables")}>
        {tables.map((table) => (
          <option key={table} value={table} />
        ))}
      </datalist>
      <div className="object-body">
        {error && <div className="notice error">{error}</div>}
        {section === "columns" && (
          <EditTable
            head={[t("Name"), t("Type"), t("Not null"), t("Default"), t("Key"), t("Comment"), ""]}
            rows={design.columns.map((c, i) => [
              <input value={c.name} onChange={(e) => setColumn(i, { name: e.target.value })} aria-label={t("Column name")} />,
              <span className="type-cell">
                <input value={c.data_type} list={id("types")} onChange={(e) => setColumn(i, { data_type: e.target.value })} aria-label={t("Type")} />
                {c.extra && <span className="muted small">{c.extra}</span>}
              </span>,
              <input type="checkbox" checked={!c.nullable || c.primary_key} disabled={c.primary_key} onChange={(e) => setColumn(i, { nullable: !e.target.checked })} aria-label={t("Not null")} />,
              <input value={c.default ?? ""} placeholder={t("none")} onChange={(e) => setColumn(i, { default: orNull(e.target.value) })} aria-label={t("Default")} />,
              <input type="checkbox" checked={c.primary_key} onChange={(e) => setColumn(i, { primary_key: e.target.checked })} aria-label={t("Primary key")} title={t("Primary key")} />,
              <input value={c.comment ?? ""} onChange={(e) => setColumn(i, { comment: orNull(e.target.value) })} aria-label={t("Comment")} />,
              <RemoveButton onClick={() => removeColumn(i)} />,
            ])}
            onAdd={() =>
              update({
                columns: [
                  ...design.columns,
                  { original: null, name: "", data_type: "", nullable: true, default: null, primary_key: false, comment: null },
                ],
              })
            }
            addLabel={t("Add column")}
          />
        )}
        {section === "indexes" && (
          <EditTable
            head={[t("Name"), t("Columns"), t("Unique"), ""]}
            rows={design.indexes.map((x, i) => [
              <input value={x.name} onChange={(e) => setPart("indexes", i, { name: e.target.value })} aria-label={t("Index name")} />,
              <NamesInput value={x.columns} onChange={(columns) => setPart("indexes", i, { columns })} hint={columnNames} />,
              <input type="checkbox" checked={x.unique} onChange={(e) => setPart("indexes", i, { unique: e.target.checked })} aria-label={t("Unique")} />,
              <RemoveButton onClick={() => removePart("indexes", i)} />,
            ])}
            onAdd={() =>
              addPart("indexes", { original: null, name: `${prefix}_idx${design.indexes.length + 1}`, columns: [], unique: false })
            }
            addLabel={t("Add index")}
          />
        )}
        {section === "foreign_keys" && (
          <EditTable
            head={[t("Name"), t("Columns"), t("Schema"), t("References"), t("Referenced columns"), t("On delete"), t("On update"), ""]}
            rows={design.foreign_keys.map((f, i) => [
              <input value={f.name} onChange={(e) => setPart("foreign_keys", i, { name: e.target.value })} aria-label={t("Foreign key name")} />,
              <NamesInput value={f.columns} onChange={(columns) => setPart("foreign_keys", i, { columns })} hint={columnNames} />,
              <input value={f.ref_schema} onChange={(e) => setPart("foreign_keys", i, { ref_schema: e.target.value })} aria-label={t("Referenced schema")} />,
              <input value={f.ref_table} list={id("tables")} onChange={(e) => setPart("foreign_keys", i, { ref_table: e.target.value })} aria-label={t("Referenced table")} />,
              <NamesInput
                value={f.ref_columns}
                onChange={(ref_columns) => setPart("foreign_keys", i, { ref_columns })}
                hint={catalog?.schemas[f.ref_schema]?.[f.ref_table] ?? []}
              />,
              <ActionSelect value={f.on_delete} onChange={(on_delete) => setPart("foreign_keys", i, { on_delete })} />,
              <ActionSelect value={f.on_update} onChange={(on_update) => setPart("foreign_keys", i, { on_update })} />,
              <RemoveButton onClick={() => removePart("foreign_keys", i)} />,
            ])}
            onAdd={() =>
              addPart("foreign_keys", {
                original: null,
                name: `${prefix}_fk${design.foreign_keys.length + 1}`,
                columns: [],
                ref_schema: tab.schema,
                ref_table: "",
                ref_columns: [],
                on_delete: null,
                on_update: null,
              })
            }
            addLabel={t("Add foreign key")}
          />
        )}
        {section === "constraints" && (
          <EditTable
            head={[t("Name"), t("Kind"), t("Columns or condition"), ""]}
            rows={design.constraints.map((c, i) => [
              <input value={c.name} onChange={(e) => setPart("constraints", i, { name: e.target.value })} aria-label={t("Constraint name")} />,
              <select value={c.kind} onChange={(e) => setPart("constraints", i, { kind: e.target.value as ConstraintDesign["kind"] })} aria-label={t("Kind")}>
                <option value="unique">UNIQUE</option>
                <option value="check">CHECK</option>
              </select>,
              c.kind === "unique" ? (
                <NamesInput value={c.columns} onChange={(columns) => setPart("constraints", i, { columns })} hint={columnNames} />
              ) : (
                <input value={c.check} placeholder="total >= 0" onChange={(e) => setPart("constraints", i, { check: e.target.value })} aria-label={t("Condition")} />
              ),
              <RemoveButton onClick={() => removePart("constraints", i)} />,
            ])}
            onAdd={() =>
              addPart("constraints", {
                original: null,
                name: `${prefix}_check${design.constraints.length + 1}`,
                kind: "check",
                columns: [],
                check: "",
              })
            }
            addLabel={t("Add constraint")}
          />
        )}
        {section === "sql" && <ScriptView preview={preview?.preview ?? null} />}
      </div>
      {confirming && ready && (
        <div className="modal-backdrop" onMouseDown={() => !running && setConfirming(false)}>
          <form
            className="modal sql-action"
            onMouseDown={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.key === "Escape" && !running && setConfirming(false)}
            onSubmit={(e) => {
              e.preventDefault();
              void apply();
            }}
          >
            <h2>{tab.table === null ? t("Create {name}", { name: design.name }) : t("Alter {name}", { name: tab.table })}</h2>
            <ScriptView preview={ready} />
            {error && <div className="notice error small">{error}</div>}
            <div className="actions">
              <button
                type="button"
                className="link"
                onClick={() => {
                  workbench().openTab("query", connection.id, { sql: ready.sql, database: tab.database });
                  setConfirming(false);
                }}
              >
                {t("Open in editor")}
              </button>
              <span className="spacer" />
              <button type="button" disabled={running} onClick={() => setConfirming(false)}>
                {t("Cancel")}
              </button>
              <button type="submit" className="primary" autoFocus disabled={running}>
                {t(running ? "Running…" : tab.table === null ? "Create" : "Apply")}
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}

/** `design` with `columns`, and `rename` applied to the columns of its other parts. */
function withColumns(design: TableDesign, columns: ColumnDesign[], rename: (cs: string[]) => string[]): TableDesign {
  return {
    ...design,
    columns,
    indexes: design.indexes.map((x) => ({ ...x, columns: rename(x.columns) })),
    foreign_keys: design.foreign_keys.map((f) => ({ ...f, columns: rename(f.columns) })),
    constraints: design.constraints.map((c) => ({ ...c, columns: rename(c.columns) })),
  };
}

function ScriptView({ preview }: { preview: Preview | null }) {
  if (!preview) return <div className="muted small">…</div>;
  if ("error" in preview) return <div className="notice error small">{preview.error}</div>;
  return (
    <>
      {preview.script.warnings.map((w) => (
        <div key={w} className="notice warn small">
          {w}
        </div>
      ))}
      <pre className="sql-preview">{preview.sql || t("No changes.")}</pre>
    </>
  );
}

function EditTable({ head, rows, onAdd, addLabel }: { head: string[]; rows: React.ReactNode[][]; onAdd: () => void; addLabel: string }) {
  return (
    <>
      <table className="meta-table edit-table">
        <thead>
          <tr>
            {head.map((h, i) => (
              <th key={i}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i}>
              {r.map((cell, j) => (
                <td key={j}>{cell}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <button className="link add-row" onClick={onAdd}>
        + {addLabel}
      </button>
    </>
  );
}

/** Column names separated by commas, read when the field loses focus. */
function NamesInput({ value, onChange, hint }: { value: string[]; onChange: (names: string[]) => void; hint: string[] }) {
  const text = value.join(", ");
  return (
    <input
      key={text}
      defaultValue={text}
      placeholder={hint.slice(0, 3).join(", ") || t("column, …")}
      title={hint.length ? t("Columns: {names}", { names: hint.join(", ") }) : undefined}
      onBlur={(e) => {
        const next = names(e.target.value);
        if (next.join(", ") !== text) onChange(next);
      }}
      aria-label={t("Columns")}
    />
  );
}

function ActionSelect({ value, onChange }: { value: string | null; onChange: (action: string | null) => void }) {
  return (
    <select value={value ?? ""} onChange={(e) => onChange(orNull(e.target.value))} aria-label={t("Referential action")}>
      <option value="">NO ACTION</option>
      {ACTIONS.map((a) => (
        <option key={a} value={a}>
          {a}
        </option>
      ))}
    </select>
  );
}

function RemoveButton({ onClick }: { onClick: () => void }) {
  return (
    <button className="icon" title={t("Remove")} aria-label={t("Remove")} onClick={onClick}>
      ×
    </button>
  );
}
