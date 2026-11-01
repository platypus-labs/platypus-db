import { registerAction } from "../actions";
import { TableEditor, type TableEditorSection } from "../table-editor/TableEditor";
import { workbench } from "../workbench";
import { editsTables, openTableEditor } from "../objects/open";
import { registerTabKind } from "./registry";
import { t } from "../i18n";

const text = (v: unknown) => (typeof v === "string" ? v : null);

registerTabKind({
  kind: "table-editor",
  component: TableEditor,
  create: (init) => ({ schema: "", table: null, ...init }),
  label: (tab) => tab.title ?? tab.table ?? t("New table"),
  persist: ({ schema, table, section, title, database }) => ({ schema, table, section, title, database }),
  restore: (saved) => ({
    schema: text(saved.schema) ?? "",
    table: text(saved.table),
    section: (text(saved.section) ?? undefined) as TableEditorSection | undefined,
    title: text(saved.title),
    database: text(saved.database),
  }),
});

registerAction({
  id: "table_editor.create",
  scope: "navigator.schema",
  title: "Create table…",
  group: "manage",
  order: 10,
  // MongoDB and Redis have no table to design.
  when: ({ connection }) => editsTables(connection.driver),
  run: ({ connection, database, schema }) =>
    workbench().openTab("table-editor", connection.id, { database, schema: schema.name, table: null }),
});

registerAction({
  id: "table_editor.edit_index",
  scope: "navigator.index",
  title: "Edit table…",
  group: "manage",
  order: 70,
  when: ({ connection, object }) => object.kind === "table" && editsTables(connection.driver),
  run: ({ connection, database, schema, object }) =>
    openTableEditor(connection.id, { database, schema: schema.name, name: object.name }),
});

registerAction({
  id: "table_editor.edit",
  scope: "navigator.object",
  title: "Edit table…",
  shortcut: "F4",
  group: "manage",
  order: 62,
  when: ({ connection, object }) => object.kind === "table" && editsTables(connection.driver),
  run: ({ connection, database, schema, object }) =>
    openTableEditor(connection.id, { database, schema: schema.name, name: object.name }),
});
