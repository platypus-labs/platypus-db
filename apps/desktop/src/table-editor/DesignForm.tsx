import type { ReactNode } from "react";
import type { ColumnDesign, ConstraintDesign, DriverKind, TableDesign } from "../api";
import { t, msg } from "../i18n";
import { Icon } from "../ui/Icon";

export type TableEditorSection = "columns" | "indexes" | "foreign_keys" | "constraints" | "sql";

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

/** A new table: one integer primary key column named `id`. */
export const blankDesign = (): TableDesign => ({
  name: "",
  columns: [
    { original: null, name: "id", data_type: "INTEGER", nullable: false, default: null, primary_key: true, comment: null },
  ],
  indexes: [],
  foreign_keys: [],
  constraints: [],
});

type PartKey = "indexes" | "foreign_keys" | "constraints";

const orNull = (s: string) => (s.trim() === "" ? null : s);
const names = (text: string) =>
  text
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);


interface Props {
  driver: DriverKind;
  design: TableDesign;
  /** Receives the function that makes the next design from the current one. */
  onChange: (change: (design: TableDesign) => TableDesign) => void;
  /** The schema the table is in: the default of a new foreign key. */
  schema: string;
  /** The tables of each schema with their columns, for the hints; `undefined` when not known. */
  catalog: Record<string, Record<string, string[]>> | undefined;
  section: TableEditorSection;
  onSection: (section: TableEditorSection) => void;
  /** Gives the "SQL" section when set: the preview of the script. */
  sql?: ReactNode;
  /** Shown above the sections, e.g. an error. */
  notice?: ReactNode;
  /** Makes the ids of the hint lists unique among the forms on the page. */
  idPrefix: string;
}

/** The form of a table design: its columns, indexes, foreign keys and constraints (and the
 *  script when `sql` is given), shared by the table editor and the ER diagram's edit mode. */
export function DesignForm({ driver, design, onChange, schema, catalog, section, onSection, sql, notice, idPrefix }: Props) {
  const id = (what: string) => `${idPrefix}-${what}`;
  const columnNames = design.columns.map((c) => c.name).filter(Boolean);
  const tables = Object.keys(catalog?.[schema] ?? {});
  const prefix = design.name || "table";
  const sections = (Object.keys(SECTION_LABELS) as TableEditorSection[]).filter((s) => s !== "sql" || sql !== undefined);

  function update(patch: Partial<TableDesign>) {
    onChange((d) => ({ ...d, ...patch }));
  }

  /** Changes column `i`; a rename follows into the indexes, keys and constraints. */
  function setColumn(i: number, patch: Partial<ColumnDesign>) {
    onChange((d) => {
      const old = d.columns[i]!;
      const column = { ...old, ...patch };
      if (patch.primary_key) column.nullable = false;
      const columns = d.columns.map((c, j) => (j === i ? column : c));
      if (column.name === old.name) return { ...d, columns };
      return withColumns(d, columns, (cs) => cs.map((c) => (c === old.name ? column.name : c)));
    });
  }

  function removeColumn(i: number) {
    onChange((d) => {
      const name = d.columns[i]!.name;
      return withColumns(
        d,
        d.columns.filter((_, j) => j !== i),
        (cs) => cs.filter((c) => c !== name),
      );
    });
  }

  function setPart<K extends PartKey>(key: K, i: number, patch: Partial<TableDesign[K][number]>) {
    onChange((d) => ({ ...d, [key]: d[key].map((p, j) => (j === i ? { ...p, ...patch } : p)) }));
  }

  function removePart(key: PartKey, i: number) {
    onChange((d) => ({ ...d, [key]: d[key].filter((_, j) => j !== i) }));
  }

  function addPart<K extends PartKey>(key: K, part: TableDesign[K][number]) {
    onChange((d) => ({ ...d, [key]: [...d[key], part] }));
  }

  return (
    <>
      <nav className="object-sections">
        {sections.map((s) => (
          <button key={s} className={s === section ? "active" : ""} onClick={() => onSection(s)}>
            {t(SECTION_LABELS[s])}
            {s !== "sql" && <span className="count">{s === "columns" ? design.columns.length : design[s].length}</span>}
          </button>
        ))}
      </nav>
      <datalist id={id("types")}>
        {(TYPES[driver] ?? []).map((type) => (
          <option key={type} value={type} />
        ))}
      </datalist>
      <datalist id={id("tables")}>
        {tables.map((table) => (
          <option key={table} value={table} />
        ))}
      </datalist>
      <div className="object-body">
        {notice}
        {section === "columns" && (
          <EditTable
            head={[t("Name"), t("Type"), t("Not null"), t("Default"), t("Key"), t("Comment"), ""]}
            rows={design.columns.map((c, i) => [
              <input value={c.name} onChange={(e) => setColumn(i, { name: e.target.value })} aria-label={t("Column name")} />,
              <span className="type-cell">
                <input value={c.data_type} list={id("types")} onChange={(e) => setColumn(i, { data_type: e.target.value })} aria-label={t("Type")} />
                {c.extra && <span className="muted small">{c.extra}</span>}
                {c.generated && <span className="muted small" title={c.generated}>{t("generated")}</span>}
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
              <NamesInput value={x.columns} onChange={(columns) => setPart("indexes", i, { columns, parts: [] })} hint={columnNames} />,
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
                hint={catalog?.[f.ref_schema]?.[f.ref_table] ?? []}
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
                ref_schema: schema,
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
        {section === "sql" && sql}
      </div>
    </>
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
      <Icon name="close" />
    </button>
  );
}
