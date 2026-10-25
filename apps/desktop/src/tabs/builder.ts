import { registerAction } from "../actions";
import { type BuilderTable, QueryBuilder } from "../builder/QueryBuilder";
import { workbench } from "../workbench";
import { registerTabKind } from "./registry";
import { t } from "../i18n";

const text = (v: unknown) => (typeof v === "string" ? v : null);

function tables(v: unknown): BuilderTable[] {
  if (!Array.isArray(v)) return [];
  return v.filter(
    (t): t is BuilderTable =>
      typeof t?.id === "number" &&
      typeof t.schema === "string" &&
      typeof t.name === "string" &&
      typeof t.alias === "string" &&
      typeof t.x === "number" &&
      typeof t.y === "number" &&
      Array.isArray(t.columns),
  );
}

registerTabKind({
  kind: "builder",
  component: QueryBuilder,
  create: (init) => ({ schema: null, tables: [], leftJoins: [], where: "", ...init }),
  label: () => t("Query builder"),
  persist: ({ schema, tables, leftJoins, where, database }) => ({ schema, tables, leftJoins, where, database }),
  restore: (saved) => ({
    schema: text(saved.schema),
    tables: tables(saved.tables),
    leftJoins: Array.isArray(saved.leftJoins) ? saved.leftJoins.filter((k): k is string => typeof k === "string") : [],
    where: text(saved.where) ?? "",
    database: text(saved.database),
  }),
});

registerAction({
  id: "builder.open_schema",
  scope: "navigator.schema",
  title: "Visual query builder",
  group: "open",
  order: 15,
  // The builder writes SQL.
  when: ({ connection }) => connection.driver !== "mongo",
  run: ({ connection, database, schema }) => workbench().openTab("builder", connection.id, { database, schema: schema.name }),
});

registerAction({
  id: "builder.open_table",
  scope: "navigator.object",
  title: "Visual query builder",
  group: "open",
  order: 30,
  when: ({ connection, object }) =>
    connection.driver !== "mongo" && (object.kind === "table" || object.kind === "view"),
  run: ({ connection, database, schema, object }) =>
    workbench().openTab("builder", connection.id, {
      database,
      schema: schema.name,
      tables: [{ id: Date.now(), schema: schema.name, name: object.name, alias: object.name[0]?.toLowerCase() ?? "t", x: 20, y: 20, columns: [] }],
    }),
});
