import { HistoryTab } from "../schema/HistoryTab";
import { t } from "../i18n";
import { registerTabKind } from "./registry";

registerTabKind({
  kind: "schema-history",
  component: HistoryTab,
  create: (init) => ({ schema: "", ...init }),
  label: (tab) => tab.title ?? t("History of {schema}", { schema: tab.schema }),
  persist: ({ schema, title, database }) => ({ schema, title, database }),
  restore: (saved) => ({
    schema: typeof saved.schema === "string" ? saved.schema : "",
    title: typeof saved.title === "string" ? saved.title : null,
    database: typeof saved.database === "string" ? saved.database : null,
  }),
});
