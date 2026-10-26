import { QueryTab, type QueryTabModel } from "../components/QueryTab";
import type { TableRef } from "../grid/edit/meta";
import type { Sort } from "../sqlNames";
import { basename } from "../workspace";
import { registerTabKind } from "./registry";

const text = (v: unknown) => (typeof v === "string" ? v : null);

/** A saved object of strings, e.g. the variables; anything else is dropped. */
function strings(v: unknown): Record<string, string> | undefined {
  if (!v || typeof v !== "object") return undefined;
  return Object.fromEntries(Object.entries(v).filter((e): e is [string, string] => typeof e[1] === "string"));
}

function tableRef(v: unknown): TableRef | null {
  if (!v || typeof v !== "object") return null;
  const { schema, name, kind } = v as Record<string, unknown>;
  return typeof schema === "string" && typeof name === "string" && typeof kind === "string"
    ? { schema, name, kind: kind as TableRef["kind"] }
    : null;
}

registerTabKind({
  kind: "query",
  component: QueryTab,
  create: (init) => {
    const sql = init.sql ?? "";
    const filePath = init.filePath ?? null;
    // A data viewer opens with its own query; a restored one brings the one it saved, if any.
    const viewerQuery = init.viewerQuery !== undefined ? init.viewerQuery : init.table ? sql : null;
    return { savedSql: filePath ? sql : null, ...init, sql, filePath, viewerQuery };
  },
  label: (tab: QueryTabModel) => {
    if (!tab.filePath) return tab.title ?? "";
    return `${basename(tab.filePath)}${tab.sql !== tab.savedSql ? " •" : ""}`;
  },
  persist: ({ sql, filePath, savedSql, title, table, tableRef, sort, viewerQuery, database, variables, terminal }) => ({
    database,
    sql,
    filePath,
    savedSql,
    title,
    table,
    tableRef,
    sort,
    viewerQuery,
    variables,
    terminal,
  }),
  restore: (saved) => ({
    sql: text(saved.sql) ?? "",
    filePath: text(saved.filePath),
    savedSql: text(saved.savedSql),
    title: text(saved.title),
    table: text(saved.table),
    tableRef: tableRef(saved.tableRef),
    sort: (saved.sort as Sort | null | undefined) ?? null,
    viewerQuery: text(saved.viewerQuery),
    database: text(saved.database),
    variables: strings(saved.variables),
    terminal: saved.terminal === true,
  }),
});
