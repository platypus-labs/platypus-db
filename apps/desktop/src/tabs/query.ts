import { QueryTab, type QueryTabModel } from "../components/QueryTab";
import type { TableRef } from "../grid/edit/meta";
import { savedSorts } from "../components/viewerSort";
import { savedCharts } from "../analysis/chartConfig";
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
  persist: ({ sql, filePath, savedSql, title, table, tableRef, sorts, viewerQuery, database, variables, unconfirmedVariables, terminal, charts }) => ({
    database,
    sql,
    filePath,
    savedSql,
    title,
    table,
    tableRef,
    sorts,
    viewerQuery,
    variables,
    unconfirmedVariables,
    terminal,
    charts,
  }),
  restore: (saved) => ({
    sql: text(saved.sql) ?? "",
    filePath: text(saved.filePath),
    savedSql: text(saved.savedSql),
    title: text(saved.title),
    table: text(saved.table),
    tableRef: tableRef(saved.tableRef),
    // `sort`: the one sort saved before each statement kept its own.
    sorts: savedSorts(saved.sorts, saved.sort),
    viewerQuery: text(saved.viewerQuery),
    database: text(saved.database),
    variables: strings(saved.variables),
    // A shared query's values still to confirm stay so after a restart.
    unconfirmedVariables: Array.isArray(saved.unconfirmedVariables)
      ? saved.unconfirmedVariables.filter((k): k is string => typeof k === "string")
      : undefined,
    terminal: saved.terminal === true,
    charts: savedCharts(saved.charts),
  }),
});
