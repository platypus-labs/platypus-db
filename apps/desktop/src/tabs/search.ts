import { SearchTab, type SearchMode } from "../search/SearchTab";
import { registerTabKind } from "./registry";
import { t } from "../i18n";

const text = (v: unknown) => (typeof v === "string" ? v : null);

registerTabKind({
  kind: "search",
  component: SearchTab,
  create: (init) => ({ mode: "metadata" as SearchMode, query: "", schema: null, ...init }),
  label: (tab) => t(tab.mode === "data" ? "Search data" : "Search"),
  persist: ({ mode, query, schema, database }) => ({ mode, query, schema, database }),
  restore: (saved) => ({
    mode: (saved.mode === "data" ? "data" : "metadata") as SearchMode,
    query: text(saved.query) ?? "",
    schema: text(saved.schema),
    database: text(saved.database),
  }),
});
