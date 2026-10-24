import type { ColumnInfo, ColumnMeta } from "../api";
import type { TableRef } from "../grid/edit/meta";

interface Props {
  /** The focused result column; `undefined` before a cell is focused. */
  column: ColumnInfo | undefined;
  /** The table behind the result, when the result is one table's rows. */
  table: TableRef | null;
  /** The table column behind the result column; `undefined` for expressions. */
  meta: ColumnMeta | undefined;
  /** The result column is part of the key that identifies a row. */
  inKey: boolean;
  onClose: () => void;
}

/** What the database says about the focused column: type, nullability, origin. */
export function MetadataPanel({ column, table, meta, inKey, onClose }: Props) {
  const rows: [string, string | null][] = column
    ? [
        ["Type", meta?.data_type ?? column.db_type],
        ["Table", table && meta ? `${table.schema}.${table.name}` : null],
        ["Column", meta?.name ?? null],
        ["Nullable", meta ? (meta.nullable ? "yes" : "no") : null],
        ["Default", meta?.default ?? null],
        ["Primary key", meta ? (meta.primary_key ? "yes" : "no") : null],
        ["Row key", table ? (inKey ? "yes" : "no") : null],
        ["Values", meta?.enum_values.length ? meta.enum_values.join(", ") : null],
        ["Fields", meta?.fields.length ? meta.fields.map((f) => `${f.name} ${f.data_type}`).join(", ") : null],
        ["Comment", meta?.comment ?? null],
      ]
    : [];
  return (
    <aside className="value-panel metadata-panel">
      <div className="value-bar">
        <strong className="small value-name" title={column?.name}>
          {column ? column.name : "Column metadata"}
        </strong>
        <span className="spacer" />
        <button className="icon" title="Close" onClick={onClose}>
          ×
        </button>
      </div>
      {!column && <div className="muted small value-empty">Select a cell to see its column.</div>}
      {column && (
        <dl className="metadata-list small">
          {rows
            .filter(([, value]) => value !== null)
            .map(([label, value]) => (
              <div key={label}>
                <dt>{label}</dt>
                <dd>{value}</dd>
              </div>
            ))}
        </dl>
      )}
      {column && !meta && (
        <div className="muted small value-size">
          {table ? "Computed in the query: not a column of the table." : "Not known to come from one table."}
        </div>
      )}
    </aside>
  );
}
