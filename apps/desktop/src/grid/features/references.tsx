// Foreign keys as links: "Open referenced row" opens the row a key points to in a data viewer
// tab, and the References panel lists the rows of other tables that point at the focused row.
import { useEffect, useMemo, useState } from "react";
import { registerAction } from "../../actions";
import {
  api,
  type ColumnInfo,
  type ColumnMeta,
  type ForeignKeyMeta,
  type ReferencingKey,
  type SavedConnection,
  toCommandError,
} from "../../api";
import { quoteIdent, viewerSql } from "../../sqlNames";
import { hiddenKeyOf } from "../../objects/open";
import { workbench } from "../../workbench";
import type { Cell, GridActionContext } from "../DataGrid";
import { type EditSource, resultColumns } from "../edit/meta";
import type { TableEditTarget } from "../edit/TableEditor";
import { t, plural } from "../../i18n";
import { EmptyState } from "../../ui/State";
import { Icon } from "../../ui/Icon";

/** Foreign keys of a result's table, for grid actions in `source.references`. */
interface GridReferences {
  /** The foreign key `col` belongs to, when all its columns are in the result. */
  foreignKey(col: number): ForeignKeyMeta | null;
  /** Opens the row a cell's foreign key points to; `cell.row` is a result row. */
  openReferenced(cell: Cell): Promise<void>;
  panelOpen: boolean;
  togglePanel(): void;
}

declare module "../DataGrid" {
  interface GridSourceExtensions {
    references?: GridReferences;
  }
}

/** Opens a data viewer tab on `schema.table` of `database` (`null`: the connection's own),
 *  filtered. */
async function openTable(
  connection: SavedConnection,
  database: string | null | undefined,
  schema: string,
  table: string,
  filter: string,
) {
  const d = connection.driver;
  const target = `${quoteIdent(schema, d)}.${quoteIdent(table, d)}`;
  const hiddenKey = await hiddenKeyOf(connection, database, schema, table);
  workbench().openTab("query", connection.id, {
    database: database ?? null,
    sql: viewerSql(d, target, filter, hiddenKey),
    autorun: true,
    title: table,
    table: target,
    tableRef: { schema, name: table, kind: "table" },
  });
}

/** `a = 1 AND b = 'x'`, or `null` when a value is NULL (it matches no row). */
function condition(driver: SavedConnection["driver"], names: string[], literals: string[]): string | null {
  if (literals.some((l) => l === "NULL") || literals.length !== names.length) return null;
  return names.map((n, i) => `${quoteIdent(n, driver)} = ${literals[i]}`).join(" AND ");
}

/** The result column of each table column name. */
function indexOf(mapped: (ColumnMeta | undefined)[]) {
  const index = new Map(mapped.flatMap((c, i) => (c ? [[c.name, i] as const] : [])));
  return (name: string) => index.get(name);
}

async function literals(jobId: number, row: number, cols: number[]) {
  return Promise.all(cols.map(async (c) => (await api.sqlLiterals(jobId, row, 1, c))[0] ?? "NULL"));
}

interface Options {
  edit: TableEditTarget | null;
  jobId: number;
  columns: ColumnInfo[];
  /** Result row of the focused cell; `null` for none or a new row. */
  row: number | null;
  onError: (message: string) => void;
}

export function useReferences({ edit, jobId, columns, row, onError }: Options) {
  const [source, setSource] = useState<EditSource | null>(null);
  const [panelOpen, setPanelOpen] = useState(false);
  useEffect(() => {
    let live = true;
    setSource(null);
    edit?.load().then(
      (s) => live && setSource(s),
      () => live && setSource(null),
    );
    return () => {
      live = false;
    };
  }, [edit?.key]);

  const mapped = useMemo(() => {
    const m = source && resultColumns(source, columns);
    return Array.isArray(m) ? m : null;
  }, [source, columns]);

  const foreignKey = (col: number) => {
    const name = mapped?.[col]?.name;
    if (!source || !mapped || name === undefined) return null;
    const at = indexOf(mapped);
    return (
      source.details.foreign_keys.find(
        (fk) => fk.columns.includes(name) && fk.columns.every((c) => at(c) !== undefined) && fk.ref_columns.every(Boolean),
      ) ?? null
    );
  };

  const references: GridReferences | undefined =
    edit && mapped
      ? {
          foreignKey,
          openReferenced: async (cell) => {
            const fk = foreignKey(cell.col);
            if (!fk || !mapped) return;
            const at = indexOf(mapped);
            const values = await literals(jobId, cell.row, fk.columns.map((c) => at(c)!));
            const filter = condition(edit.connection.driver, fk.ref_columns, values);
            if (filter === null) return onError(t("The foreign key is NULL: it points to no row."));
            openTable(edit.connection, edit.database, fk.ref_schema, fk.ref_table, filter);
          },
          panelOpen,
          togglePanel: () => setPanelOpen((o) => !o),
        }
      : undefined;

  // `null` when closed (not `false`): the grid shows the side panels' handle only for a panel.
  const panel =
    panelOpen && edit && source && mapped ? (
      <ReferencesPanel
        edit={edit}
        source={source}
        mapped={mapped}
        jobId={jobId}
        row={row}
        onClose={() => setPanelOpen(false)}
      />
    ) : null;
  return { references, panel };
}

interface Incoming {
  key: ReferencingKey;
  /** `null` while counting, or when the row's key is NULL. */
  count: string | null;
  filter: string | null;
}

/** Waits for the focused row to settle before counting. */
const COUNT_DELAY_MS = 200;

function ReferencesPanel({
  edit,
  source,
  mapped,
  jobId,
  row,
  onClose,
}: {
  edit: TableEditTarget;
  source: EditSource;
  mapped: (ColumnMeta | undefined)[];
  jobId: number;
  row: number | null;
  onClose: () => void;
}) {
  const [keys, setKeys] = useState<ReferencingKey[] | null>(null);
  const [incoming, setIncoming] = useState<Incoming[]>([]);
  const [error, setError] = useState<string | null>(null);
  const { schema, name } = source.table;
  const driver = edit.connection.driver;

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const id = await edit.session();
        // No session (the password prompt was cancelled, the connection failed): say so
        // instead of an empty panel.
        if (id === null) {
          if (live) setError(t("Not connected."));
          return;
        }
        const found = await api.referencingKeys(id, schema, name);
        if (live) setKeys(found);
      } catch (e) {
        if (live) setError(toCommandError(e).message);
      }
    })();
    return () => {
      live = false;
    };
  }, [schema, name]);

  useEffect(() => {
    if (!keys || row === null) return setIncoming([]);
    let live = true;
    const at = indexOf(mapped);
    const timer = setTimeout(async () => {
      try {
        const id = await edit.session();
        if (id === null || !live) return;
        const rows = await Promise.all(
          keys.map(async (key): Promise<Incoming> => {
            const cols = key.ref_columns.map((c) => at(c));
            if (cols.some((c) => c === undefined)) return { key, count: null, filter: null };
            const filter = condition(driver, key.columns, await literals(jobId, row, cols as number[]));
            if (filter === null) return { key, count: "0", filter: null };
            const target = `${quoteIdent(key.schema, driver)}.${quoteIdent(key.table, driver)}`;
            const count = await api.queryValue(id, `SELECT COUNT(*) FROM ${target} WHERE ${filter}`);
            return { key, count, filter };
          }),
        );
        if (live) setIncoming(rows);
      } catch (e) {
        if (live) setError(toCommandError(e).message);
      }
    }, COUNT_DELAY_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [keys, row, jobId, mapped]);

  return (
    <aside className="value-panel references-panel">
      <div className="value-bar">
        <strong className="small value-name">{t("References to {name}", { name })}</strong>
        <span className="spacer" />
        <button className="icon" title={t("Close")} aria-label={t("Close")} onClick={onClose}>
          <Icon name="close" />
        </button>
      </div>
      {error && <div className="error-text small value-empty">{error}</div>}
      {keys?.length === 0 && <EmptyState compact className="value-empty" title={t("No foreign key points at {name}.", { name })} />}
      {keys && keys.length > 0 && row === null && (
        <EmptyState compact className="value-empty" title={t("Focus a row of the result.")} />
      )}
      <div className="references-list">
        {row !== null &&
          incoming.map(({ key, count, filter }) => (
            <div key={`${key.schema}.${key.table}.${key.name}`} className="reference">
              <div className="small">
                <strong>{key.table}</strong>
                <span className="muted"> ({key.columns.join(", ")})</span>
              </div>
              <span className="spacer" />
              {filter === null ? (
                <span className="muted small" title={t("The key columns are not all in the result")}>
                  {count === "0" ? plural(0, "{n} row", "{n} rows") : "–"}
                </span>
              ) : (
                <button
                  className="link small"
                  onClick={() => openTable(edit.connection, edit.database, key.schema, key.table, filter)}
                  title={t("Opens {table} WHERE {filter}", { table: key.table, filter })}
                >
                  {count === null ? t("? rows") : plural(Number(count), "{n} row", "{n} rows")}
                </button>
              )}
            </div>
          ))}
      </div>
    </aside>
  );
}

const references = ({ source }: GridActionContext) => source?.references;

registerAction({
  id: "grid.open_reference",
  scope: "grid",
  title: "Open referenced row",
  group: "references",
  order: 90,
  when: (ctx) => {
    const f = ctx.grid.focused();
    return !!f && ctx.grid.dataRow(f.row) !== null && !!references(ctx)?.foreignKey(f.col);
  },
  run: (ctx) => {
    const f = ctx.grid.focused()!;
    return references(ctx)!.openReferenced({ row: ctx.grid.dataRow(f.row)!, col: f.col });
  },
});

registerAction({
  id: "grid.references_panel",
  scope: "grid",
  title: "References panel",
  group: "references",
  order: 91,
  when: (ctx) => references(ctx) !== undefined,
  run: (ctx) => references(ctx)!.togglePanel(),
});
