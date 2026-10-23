import { QueryTab, type QueryTabModel } from "../components/QueryTab";
import type { Sort } from "../sqlNames";
import { basename } from "../workspace";
import { registerTabKind } from "./registry";

const text = (v: unknown) => (typeof v === "string" ? v : null);

registerTabKind({
  kind: "query",
  component: QueryTab,
  create: (init) => {
    const sql = init.sql ?? "";
    const filePath = init.filePath ?? null;
    return { savedSql: filePath ? sql : null, ...init, sql, filePath };
  },
  label: (tab: QueryTabModel) => {
    if (!tab.filePath) return tab.title ?? "";
    return `${basename(tab.filePath)}${tab.sql !== tab.savedSql ? " •" : ""}`;
  },
  persist: ({ sql, filePath, savedSql, title, table, filter, sort, database }) => ({
    database,
    sql,
    filePath,
    savedSql,
    title,
    table,
    filter,
    sort,
  }),
  restore: (saved) => ({
    sql: text(saved.sql) ?? "",
    filePath: text(saved.filePath),
    savedSql: text(saved.savedSql),
    title: text(saved.title),
    table: text(saved.table),
    filter: text(saved.filter) ?? undefined,
    sort: (saved.sort as Sort | null | undefined) ?? null,
    database: text(saved.database),
  }),
});
