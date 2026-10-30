import type { ColumnInfo, ColumnMeta } from "../api";
import type { TableRef } from "../grid/edit/meta";
import { t } from "../i18n";
import { EmptyState } from "../ui/State";
import { Icon } from "../ui/Icon";

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
        [t("Type"), meta?.data_type ?? column.db_type],
        [t("Table"), table && meta ? `${table.schema}.${table.name}` : null],
        [t("Column"), meta?.name ?? null],
        [t("Nullable"), meta ? t(meta.nullable ? "yes" : "no") : null],
        [t("Default"), meta?.default ?? null],
        [t("Primary key"), meta ? t(meta.primary_key ? "yes" : "no") : null],
        [t("Row key"), table ? t(inKey ? "yes" : "no") : null],
        [t("Values"), meta?.enum_values.length ? meta.enum_values.join(", ") : null],
        [t("Fields"), meta?.fields.length ? meta.fields.map((f) => `${f.name} ${f.data_type}`).join(", ") : null],
        [t("Comment"), meta?.comment ?? null],
      ]
    : [];
  return (
    <aside className="value-panel metadata-panel">
      <div className="value-bar">
        <strong className="small value-name" title={column?.name}>
          {column ? column.name : t("Column metadata")}
        </strong>
        <span className="spacer" />
        <button className="icon" title={t("Close")} aria-label={t("Close")} onClick={onClose}>
          <Icon name="close" />
        </button>
      </div>
      {!column && <EmptyState compact className="value-empty" title={t("Select a cell to see its column.")} />}
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
          {t(table ? "Computed in the query: not a column of the table." : "Not known to come from one table.")}
        </div>
      )}
    </aside>
  );
}
