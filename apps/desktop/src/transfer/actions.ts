// Transfers from the navigator: data files into an existing or a new table, a table to another
// database, tables to files.
import { createElement } from "react";
import { registerAction } from "../actions";
import { api, opensDataFile, type SavedConnection, type SchemaMeta, toCommandError } from "../api";
import { isRelation } from "../components/Navigator";
import { workbench } from "../workbench";
import { ExportTablesDialog } from "./ExportTablesDialog";
import { GenerateDataDialog } from "./GenerateDataDialog";
import { ImportDataDialog, type TableAt } from "./ImportDataDialog";
import { MigrateDialog } from "./MigrateDialog";

const at = (connection: SavedConnection, database: string | null, schema: SchemaMeta, table: string | null): TableAt => ({
  connection,
  database,
  schema: schema.name,
  currentSchema: schema.current,
  table,
});

function openImport(connection: SavedConnection, database: string | null, schema: SchemaMeta, table: string | null) {
  workbench().openDialog((close) =>
    createElement(ImportDataDialog, { target: at(connection, database, schema, table), onClose: close }),
  );
}

registerAction({
  id: "object.transfer",
  scope: "navigator.object",
  title: "Copy to a database…",
  group: "manage",
  order: 72,
  when: ({ object }) => isRelation(object.kind),
  run: ({ connection, database, schema, object }) => {
    // Another connection by default, in its current schema; else a copy next to the table.
    const other = workbench()
      .connections()
      .find((c) => c.id !== connection.id && !opensDataFile(c));
    const target: TableAt = other
      ? { connection: other, database: null, schema: "", currentSchema: true, table: null }
      : at(connection, database, schema, null);
    workbench().openDialog((close) =>
      createElement(ImportDataDialog, {
        source: { ...at(connection, database, schema, object.name), table: object.name },
        target,
        newTable: other ? object.name : `${object.name}_copy`,
        onClose: close,
      }),
    );
  },
});

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

registerAction({
  id: "schema.paste_table",
  scope: "navigator.schema",
  title: "Paste CSV as a new table…",
  group: "open",
  order: 32,
  run: async ({ connection, database, schema }) => {
    let path: string;
    try {
      path = await api.clipboardToFile();
    } catch (e) {
      return workbench().showError(toCommandError(e).message);
    }
    workbench().openDialog((close) =>
      createElement(ImportDataDialog, {
        target: at(connection, database, schema, null),
        path,
        onClose: () => {
          close();
          void api.discardPastedFile(path);
        },
      }),
    );
  },
});

function openMigrate(connection: SavedConnection, database: string | null, schema: string, tables: string[] | null) {
  workbench().openDialog((close) => createElement(MigrateDialog, { connection, database, schema, tables, onClose: close }));
}

registerAction({
  id: "object.migrate",
  scope: "navigator.object",
  title: "Copy tables to a database…",
  group: "manage",
  order: 73,
  multiple: true,
  // One table has "Copy to a database…", with its columns to map.
  when: ({ schema, selection }) =>
    (selection?.length ?? 0) > 1 && selection!.every((s) => s.object.kind === "table" && s.schema.name === schema.name),
  run: ({ connection, database, schema, selection }) =>
    openMigrate(connection, database, schema.name, selection!.map((s) => s.object.name)),
});

registerAction({
  id: "schema.migrate",
  scope: "navigator.schema",
  title: "Copy schema to a database…",
  group: "open",
  order: 33,
  run: ({ connection, database, schema }) => openMigrate(connection, database, schema.name, null),
});

registerAction({
  id: "object.generate_data",
  scope: "navigator.object",
  title: "Generate data…",
  group: "manage",
  order: 74,
  when: ({ object }) => object.kind === "table",
  run: ({ connection, database, schema, object }) =>
    workbench().openDialog((close) =>
      createElement(GenerateDataDialog, {
        connection,
        database,
        schema: schema.name,
        currentSchema: schema.current,
        table: object.name,
        onClose: close,
      }),
    ),
});
