import { registerAction } from "../actions";
import { isNoSql, type ObjectKind } from "../api";
import { TableProfileTab } from "../analysis/TableProfileTab";
import { DEFAULT_SAMPLE, restoreSample } from "../analysis/tableProfileModel";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { registerTabKind } from "./registry";

const text = (v: unknown) => (typeof v === "string" ? v : null);

/** The objects whose rows can be profiled. */
const PROFILED: ObjectKind[] = ["table", "view", "materialized_view", "foreign_table"];

registerTabKind({
  kind: "table-profile",
  component: TableProfileTab,
  create: (init) => ({
    schema: "",
    currentSchema: false,
    name: "",
    objectKind: "table" as ObjectKind,
    sample: DEFAULT_SAMPLE,
    ...init,
  }),
  label: (tab) => tab.title ?? t("Profile of {table}", { table: tab.name }),
  // The profile itself is read again when the tab opens: the table may have changed.
  persist: ({ schema, currentSchema, name, objectKind, sample, title, database }) => ({
    schema,
    currentSchema,
    name,
    objectKind,
    sample,
    title,
    database,
  }),
  restore: (saved) => ({
    schema: text(saved.schema) ?? "",
    currentSchema: saved.currentSchema === true,
    name: text(saved.name) ?? "",
    objectKind: (text(saved.objectKind) ?? "table") as ObjectKind,
    sample: restoreSample(saved.sample),
    title: text(saved.title),
    database: text(saved.database),
  }),
});

registerAction({
  id: "object.profile_data",
  scope: "navigator.object",
  title: "Profile data…",
  group: "open",
  order: 20,
  when: ({ connection, object }) => !isNoSql(connection.driver) && PROFILED.includes(object.kind),
  run: ({ connection, database, schema, object }) =>
    workbench().openTab("table-profile", connection.id, {
      database,
      schema: schema.name,
      currentSchema: schema.current,
      name: object.name,
      objectKind: object.kind,
      title: t("Profile of {table}", { table: object.name }),
    }),
});
