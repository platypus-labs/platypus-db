import { registerAction } from "../actions";
import { TableEditor, type TableEditorSection } from "../table-editor/TableEditor";
import { workbench } from "../workbench";
import { registerTabKind } from "./registry";

const text = (v: unknown) => (typeof v === "string" ? v : null);

registerTabKind({
  kind: "table-editor",
  component: TableEditor,
  create: (init) => ({ schema: "", table: null, ...init }),
  label: (tab) => tab.title ?? tab.table ?? "New table",
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
  run: ({ connection, database, schema }) =>
    workbench().openTab("table-editor", connection.id, { database, schema: schema.name, table: null }),
});

registerAction({
  id: "table_editor.edit",
  scope: "navigator.object",
  title: "Edit table…",
  group: "manage",
  order: 70,
  when: ({ object }) => object.kind === "table",
  run: ({ connection, database, schema, object }) =>
    workbench().openTab("table-editor", connection.id, {
      database,
      schema: schema.name,
      table: object.name,
      title: object.name,
    }),
});
