/**
 * Visual query builder: tables dragged from the list onto a canvas, joined on their foreign
 * keys, with the columns ticked in each box; the SELECT is written as they change and opens
 * in a SQL editor.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { api, type DriverKind, type ObjectMeta, type SchemaMeta, type TableDetails, toCommandError } from "../api";
import { quoteIdent } from "../sqlNames";
import type { TabBase, TabProps } from "../tabs/registry";
import { Icon } from "../ui/Icon";
import { workbench } from "../workbench";
import { t } from "../i18n";
import { EmptyState } from "../ui/State";

export interface BuilderTable {
  id: number;
  schema: string;
  name: string;
  alias: string;
  x: number;
  y: number;
  /** Ticked columns, in the table's order. */
  columns: string[];
}

export interface BuilderTabModel extends TabBase {
  kind: "builder";
  schema: string | null;
  tables: BuilderTable[];
  /** Joins made LEFT JOIN, by `joinKey`; the others are inner joins. */
  leftJoins: string[];
  where: string;
}

declare module "../tabs/registry" {
  interface TabKinds {
    builder: BuilderTabModel;
  }
}

/** A join between a table and one placed before it, on a foreign key of either. */
export interface BuilderJoin {
  key: string;
  /** The table placed earlier, and the later one that joins it. */
  left: BuilderTable;
  right: BuilderTable;
  /** Pairs of `[left column, right column]`. */
  on: [string, string][];
}

const detailsKey = (t: { schema: string; name: string }) => `${t.schema}\u0000${t.name}`;

/** The columns a foreign key references: as declared, else (SQLite `REFERENCES t`) the
 *  referenced table's primary key. */
function referenced(columns: string[], target: TableDetails | undefined): string[] {
  return columns.length ? columns : (target?.columns.filter((c) => c.primary_key).map((c) => c.name) ?? []);
}

/** The joins of `tables`: each one after the first, joined to the first earlier table one
 *  of their foreign keys links. */
export function joinsOf(tables: BuilderTable[], details: Map<string, TableDetails>): BuilderJoin[] {
  const joins: BuilderJoin[] = [];
  tables.forEach((right, i) => {
    const rd = details.get(detailsKey(right));
    for (const left of tables.slice(0, i)) {
      const ld = details.get(detailsKey(left));
      const out = rd?.foreign_keys.find((fk) => fk.ref_schema === left.schema && fk.ref_table === left.name);
      const back = ld?.foreign_keys.find((fk) => fk.ref_schema === right.schema && fk.ref_table === right.name);
      // A key whose columns cannot be told is no join: the table is cross joined.
      if (out && referenced(out.ref_columns, ld).length) {
        const refs = referenced(out.ref_columns, ld);
        joins.push({ key: `${left.id}-${right.id}-${out.name}`, left, right, on: refs.map((c, k) => [c, out.columns[k]!]) });
        return;
      }
      if (back && referenced(back.ref_columns, rd).length) {
        const refs = referenced(back.ref_columns, rd);
        joins.push({ key: `${left.id}-${right.id}-${back.name}`, left, right, on: back.columns.map((c, k) => [c, refs[k]!]) });
        return;
      }
    }
  });
  return joins;
}

/** The SELECT the canvas describes; `current` is the session's schema, left unqualified. */
export function builderSql(
  driver: DriverKind,
  model: Pick<BuilderTabModel, "tables" | "leftJoins" | "where">,
  joins: BuilderJoin[],
  current: string | null,
): string {
  const q = (n: string) => quoteIdent(n, driver);
  const table = (t: BuilderTable) => `${t.schema === current ? "" : `${q(t.schema)}.`}${q(t.name)} ${q(t.alias)}`;
  const [first, ...rest] = model.tables;
  if (!first) return "";
  const columns = model.tables.flatMap((t) => t.columns.map((c) => `${q(t.alias)}.${q(c)}`));
  let sql = `SELECT ${columns.length ? columns.join(",\n       ") : "*"}\nFROM ${table(first)}`;
  for (const t of rest) {
    const join = joins.find((j) => j.right.id === t.id);
    if (!join) {
      sql += `\nCROSS JOIN ${table(t)}`;
      continue;
    }
    const on = join.on.map(([l, r]) => `${q(join.right.alias)}.${q(r)} = ${q(join.left.alias)}.${q(l)}`).join(" AND ");
    sql += `\n${model.leftJoins.includes(join.key) ? "LEFT JOIN" : "JOIN"} ${table(t)} ON ${on}`;
  }
  if (model.where.trim()) sql += `\nWHERE ${model.where.trim()}`;
  return `${sql};`;
}

/** A short alias from the table's name, not used yet: `order_items` is `oi`. */
function aliasFor(name: string, taken: string[]): string {
  const words = name.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);
  const base = (words.length > 1 ? words.map((w) => w[0]).join("") : words[0]?.slice(0, 1)) || "t";
  let alias = base;
  for (let n = 2; taken.includes(alias); n++) alias = `${base}${n}`;
  return alias;
}

const BOX = 200;
let nextTableId = Date.now();

export function QueryBuilder({ tab, connection, active, sideSession, onChange, onError }: TabProps<BuilderTabModel>) {
  const [schemas, setSchemas] = useState<SchemaMeta[]>([]);
  const [objects, setObjects] = useState<ObjectMeta[]>([]);
  const [filter, setFilter] = useState("");
  const [details, setDetails] = useState(new Map<string, TableDetails>());
  const canvas = useRef<HTMLDivElement>(null);
  const fail = (e: unknown) => onError(toCommandError(e).message);
  const session = async () => {
    const id = await sideSession();
    if (id === null) throw new Error(t("no connection"));
    return id;
  };

  useEffect(() => {
    session()
      .then(api.listSchemas)
      .then((all) => {
        setSchemas(all);
        if (!tab.schema) onChange({ schema: all.find((s) => s.current)?.name ?? all[0]?.name ?? null });
      })
      .catch(fail);
  }, []);

  useEffect(() => {
    if (!tab.schema) return;
    session()
      .then((id) => api.listObjects(id, tab.schema!))
      .then((all) => setObjects(all.filter((o) => o.kind === "table" || o.kind === "view" || o.kind === "materialized_view")))
      .catch(fail);
  }, [tab.schema]);

  // Every placed table's columns and keys, loaded once.
  useEffect(() => {
    const missing = tab.tables.filter((t) => !details.has(detailsKey(t)));
    if (missing.length === 0) return;
    void (async () => {
      try {
        const id = await session();
        const loaded = await Promise.all(missing.map((t) => api.tableDetails(id, t.schema, t.name)));
        setDetails((d) => {
          const next = new Map(d);
          missing.forEach((t, i) => next.set(detailsKey(t), loaded[i]!));
          return next;
        });
      } catch (e) {
        fail(e);
      }
    })();
  }, [tab.tables]);

  const current = schemas.find((s) => s.current)?.name ?? null;
  const joins = useMemo(() => joinsOf(tab.tables, details), [tab.tables, details]);
  const sql = builderSql(connection.driver, tab, joins, current);
  const setTables = (tables: BuilderTable[]) => onChange({ tables });

  function add(name: string, x?: number, y?: number) {
    if (!tab.schema) return;
    const n = tab.tables.length;
    const table: BuilderTable = {
      id: nextTableId++,
      schema: tab.schema,
      name,
      alias: aliasFor(name, tab.tables.map((t) => t.alias)),
      x: x ?? 20 + (n % 4) * (BOX + 40),
      y: y ?? 20 + Math.floor(n / 4) * 260,
      columns: [],
    };
    setTables([...tab.tables, table]);
  }

  function move(e: React.MouseEvent, table: BuilderTable) {
    e.preventDefault();
    const start = { x: e.clientX, y: e.clientY };
    const onMove = (ev: MouseEvent) =>
      setTables(
        tab.tables.map((t) =>
          t.id === table.id
            ? { ...t, x: Math.max(0, table.x + ev.clientX - start.x), y: Math.max(0, table.y + ev.clientY - start.y) }
            : t,
        ),
      );
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  function toggleColumn(table: BuilderTable, column: string, all: string[]) {
    const on = table.columns.includes(column);
    const columns = all.filter((c) => (c === column ? !on : table.columns.includes(c)));
    setTables(tab.tables.map((t) => (t.id === table.id ? { ...t, columns } : t)));
  }

  const open = (autorun: boolean) =>
    workbench().openTab("query", connection.id, { sql, database: tab.database, autorun });
  const center = (t: BuilderTable) => ({ x: t.x + BOX / 2, y: t.y + 16 });
  const visible = objects.filter((o) => o.name.toLowerCase().includes(filter.toLowerCase()));

  return (
    <div className="builder-tab" style={{ display: active ? "flex" : "none" }}>
      <aside className="builder-tables">
        <select aria-label={t("Schema")} value={tab.schema ?? ""} onChange={(e) => onChange({ schema: e.target.value })}>
          {schemas.map((s) => (
            <option key={s.name} value={s.name}>
              {s.name}
            </option>
          ))}
        </select>
        <input placeholder={t("Filter tables…")} aria-label={t("Filter tables…")} value={filter} onChange={(e) => setFilter(e.target.value)} />
        <ul className="tree">
          {visible.map((o) => (
            <li
              key={o.name}
              className="tree-row builder-source"
              draggable
              title={t("Drag onto the canvas, or double-click")}
              onDragStart={(e) => e.dataTransfer.setData("application/x-platypus-table", o.name)}
              onDoubleClick={() => add(o.name)}
            >
              <span className="tree-icon" data-kind={o.kind === "table" ? "table" : "view"}>
                <Icon name={o.kind === "table" ? "table" : "view"} />
              </span>
              <span className="tree-label">{o.name}</span>
            </li>
          ))}
        </ul>
      </aside>
      <div className="builder-main">
        <div
          className="builder-canvas"
          ref={canvas}
          onDragOver={(e) => e.dataTransfer.types.includes("application/x-platypus-table") && e.preventDefault()}
          onDrop={(e) => {
            const name = e.dataTransfer.getData("application/x-platypus-table");
            if (!name) return;
            e.preventDefault();
            const r = canvas.current!.getBoundingClientRect();
            const el = canvas.current!;
            add(name, Math.max(0, e.clientX - r.left + el.scrollLeft - BOX / 2), Math.max(0, e.clientY - r.top + el.scrollTop - 12));
          }}
        >
          {tab.tables.length === 0 && <EmptyState className="builder-empty" title={t("Drag tables here from the list. Their foreign keys join them.")} />}
          <svg className="builder-lines">
            {joins.map((j) => {
              const a = center(j.left);
              const b = center(j.right);
              return <line key={j.key} x1={a.x} y1={a.y} x2={b.x} y2={b.y} />;
            })}
          </svg>
          {joins.map((j) => {
            const a = center(j.left);
            const b = center(j.right);
            const left = tab.leftJoins.includes(j.key);
            return (
              <button
                key={j.key}
                className="builder-join"
                style={{ left: (a.x + b.x) / 2, top: (a.y + b.y) / 2 }}
                title={`${j.on.map(([l, r]) => `${j.right.alias}.${r} = ${j.left.alias}.${l}`).join(" AND ")}\n${t("Click for {join}", { join: left ? "JOIN" : "LEFT JOIN" })}`}
                onClick={() =>
                  onChange({ leftJoins: left ? tab.leftJoins.filter((k) => k !== j.key) : [...tab.leftJoins, j.key] })
                }
              >
                {left ? "LEFT" : "JOIN"}
              </button>
            );
          })}
          {tab.tables.map((box) => {
            const columns = details.get(detailsKey(box))?.columns.map((c) => c.name) ?? [];
            return (
              <div key={box.id} className="builder-box" data-table={box.name} style={{ left: box.x, top: box.y, width: BOX }}>
                <div className="builder-box-header" onMouseDown={(e) => move(e, box)}>
                  <span className="tree-label">
                    {box.name} <span className="muted">{box.alias}</span>
                  </span>
                  <button
                    className="icon"
                    title={t("Remove")}
                    aria-label={t("Remove")}
                    onMouseDown={(e) => e.stopPropagation()}
                    onClick={() => setTables(tab.tables.filter((x) => x.id !== box.id))}
                  >
                    ×
                  </button>
                </div>
                <ul className="builder-columns">
                  {columns.map((c) => (
                    <li key={c}>
                      <label>
                        <input type="checkbox" checked={box.columns.includes(c)} onChange={() => toggleColumn(box, c, columns)} />
                        {c}
                      </label>
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
        </div>
        <div className="builder-bottom">
          <label className="builder-where">
            <span className="muted small">WHERE</span>
            <input value={tab.where} placeholder={t("e.g. o.total > 100")} onChange={(e) => onChange({ where: e.target.value })} />
          </label>
          <pre className="builder-sql">{sql || `-- ${t("No tables yet")}`}</pre>
          <div className="actions">
            <span className="spacer" />
            <button disabled={!sql} onClick={() => open(false)}>
              {t("Open in editor")}
            </button>
            <button className="primary" disabled={!sql} onClick={() => open(true)}>
              {t("Run")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
