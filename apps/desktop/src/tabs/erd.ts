import { registerAction } from "../actions";
import { isNoSql } from "../api";
import { ErdTab, type ErdNote } from "../erd/ErdTab";
import type { Point } from "../erd/layout";
import { workbench } from "../workbench";
import { registerTabKind } from "./registry";
import { t } from "../i18n";

const text = (v: unknown) => (typeof v === "string" ? v : null);
const isPoint = (v: unknown): v is Point =>
  typeof v === "object" && v !== null && typeof (v as Point).x === "number" && typeof (v as Point).y === "number";

registerTabKind({
  kind: "erd",
  component: ErdTab,
  create: (init) => ({ schema: "", currentSchema: false, tables: null, positions: {}, notes: [], zoom: 1, ...init }),
  label: (tab) => tab.title ?? t("ER diagram of {schema}", { schema: tab.schema }),
  persist: ({ schema, currentSchema, tables, positions, notes, zoom, title, database }) => ({
    schema,
    currentSchema,
    tables,
    positions,
    notes,
    zoom,
    title,
    database,
  }),
  restore: (saved) => ({
    schema: text(saved.schema) ?? "",
    currentSchema: saved.currentSchema === true,
    tables: Array.isArray(saved.tables) ? saved.tables.filter((n): n is string => typeof n === "string") : null,
    positions: Object.fromEntries(
      Object.entries(typeof saved.positions === "object" && saved.positions ? saved.positions : {}).filter(([, p]) => isPoint(p)),
    ),
    notes: Array.isArray(saved.notes)
      ? saved.notes.filter((n): n is ErdNote => isPoint(n) && typeof (n as ErdNote).id === "number" && typeof (n as ErdNote).text === "string")
      : [],
    zoom: typeof saved.zoom === "number" ? saved.zoom : 1,
    title: text(saved.title),
    database: text(saved.database),
  }),
});

registerAction({
  id: "schema.erd",
  scope: "navigator.schema",
  title: "ER diagram",
  group: "open",
  order: 35,
  when: ({ connection }) => !isNoSql(connection.driver),
  run: ({ connection, database, schema }) =>
    workbench().openTab("erd", connection.id, { database, schema: schema.name, currentSchema: schema.current }),
});

registerAction({
  id: "object.erd",
  scope: "navigator.object",
  title: "Show in ER diagram",
  group: "open",
  order: 40,
  when: ({ object }) => object.kind === "table",
  run: ({ connection, database, schema, object, selection }) => {
    const tables = (selection ?? [{ schema, object }]).filter((s) => s.object.kind === "table" && s.schema.name === schema.name).map((s) => s.object.name);
    workbench().openTab("erd", connection.id, { database, schema: schema.name, currentSchema: schema.current, tables });
  },
});
