// Import of data files from the navigator: into an existing table, or into a new one.
import { createElement } from "react";
import { registerAction } from "../actions";
import type { SavedConnection, SchemaMeta } from "../api";
import { workbench } from "../workbench";
import { ExportTablesDialog } from "./ExportTablesDialog";
import { ImportDataDialog } from "./ImportDataDialog";

function openImport(connection: SavedConnection, database: string | null, schema: SchemaMeta, table: string | null) {
  workbench().openDialog((close) =>
    createElement(ImportDataDialog, {
      connection,
      database,
      schema: schema.name,
      currentSchema: schema.current,
      table,
      onClose: close,
    }),
  );
}

registerAction({
  id: "object.import_data",
  scope: "navigator.object",
  title: "Import data…",
  group: "manage",
  order: 70,
  when: ({ object }) => object.kind === "table",
  run: ({ connection, database, schema, object }) => openImport(connection, database, schema, object.name),
});

registerAction({
  id: "schema.import_data",
  scope: "navigator.schema",
  title: "Import data into a new table…",
  group: "open",
  order: 30,
  run: ({ connection, database, schema }) => openImport(connection, database, schema, null),
});

function openExport(
  connection: SavedConnection,
  database: string | null,
  schema: SchemaMeta,
  tables: { schema: string | null; name: string }[] | null,
) {
  workbench().openDialog((close) =>
    createElement(ExportTablesDialog, {
      connection,
      database,
      schema: schema.name,
      currentSchema: schema.current,
      tables,
      onClose: close,
    }),
  );
}

registerAction({
  id: "object.export_tables",
  scope: "navigator.object",
  title: "Export data…",
  group: "manage",
  order: 71,
  multiple: true,
  when: ({ object, selection }) => (selection ?? [{ object }]).every((s) => s.object.kind === "table"),
  run: ({ connection, database, schema, object, selection }) =>
    openExport(connection, database, schema, (selection ?? [{ schema, object }]).map((s) => ({ schema: s.schema.current ? null : s.schema.name, name: s.object.name }))),
});

registerAction({
  id: "schema.export_tables",
  scope: "navigator.schema",
  title: "Export all tables…",
  group: "open",
  order: 31,
  run: ({ connection, database, schema }) => openExport(connection, database, schema, null),
});
