// Import of data files from the navigator: into an existing table, or into a new one.
import { createElement } from "react";
import { registerAction } from "../actions";
import type { SavedConnection, SchemaMeta } from "../api";
import { workbench } from "../workbench";
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
