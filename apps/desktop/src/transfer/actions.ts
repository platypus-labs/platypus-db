// Transfers from the navigator: data files into an existing or a new table, a table to another
// database, tables to files.
import { createElement } from "react";
import { registerAction } from "../actions";
import { api, isMongo, isNoSql, isRedis, isSearch, keepsDataIn, opensDataFile, type SavedConnection, type SchemaMeta, takesNoRows, toCommandError } from "../api";
import { isRelation } from "../components/Navigator";
import { omitsSchema, schemaRef } from "../sqlNames";
import { answerFor } from "../testAnswers";
import { workbench } from "../workbench";
import { t } from "../i18n";
import { ExportTablesDialog } from "./ExportTablesDialog";
import { GenerateDataDialog } from "./GenerateDataDialog";
import { ImportDataDialog, type TableAt } from "./ImportDataDialog";
import { MigrateDialog } from "./MigrateDialog";
import { ProfilesDialog } from "./ProfilesDialog";
import { DumpSqlDialog, RunSqlFileDialog } from "./SqlFileDialogs";

const at = (connection: SavedConnection, database: string | null, schema: SchemaMeta, table: string | null): TableAt => ({
  connection,
  database,
  schema: schema.name,
  currentSchema: omitsSchema(connection.driver, schema.current),
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
  when: ({ connection, object }) => isRelation(object.kind) && !keepsDataIn(connection),
  run: ({ connection, database, schema, object }) => {
    // Another connection by default, in its current schema; else a copy next to the table.
    // MongoDB and Redis take no SQL tables: their collections and key types go to a SQL database.
    const other = workbench()
      .connections()
      .find((c) => c.id !== connection.id && !opensDataFile(c) && !isNoSql(c.driver) && !takesNoRows(c));
    if (!other && isNoSql(connection.driver)) {
      return workbench().showError(t(isMongo(connection.driver) ? "Save a SQL connection to copy the collection to." : "Save a SQL connection to copy the table to."));
    }
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
  // MongoDB's collections take documents, not the rows of an import.
  when: ({ connection, object }) => object.kind === "table" && !isNoSql(connection.driver) && !takesNoRows(connection),
  run: ({ connection, database, schema, object }) => openImport(connection, database, schema, object.name),
});

registerAction({
  id: "schema.import_data",
  scope: "navigator.schema",
  title: "Import data into a new table…",
  group: "open",
  order: 30,
  when: ({ connection }) => !isNoSql(connection.driver) && !takesNoRows(connection),
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
      currentSchema: omitsSchema(connection.driver, schema.current),
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
  when: ({ connection, object, selection }) =>
    !keepsDataIn(connection) && (selection ?? [{ object }]).every((s) => s.object.kind === "table"),
  run: ({ connection, database, schema, object, selection }) =>
    openExport(connection, database, schema, (selection ?? [{ schema, object }]).map((s) => ({ schema: schemaRef(connection.driver, s.schema), name: s.object.name }))),
});

registerAction({
  id: "schema.export_tables",
  scope: "navigator.schema",
  title: "Export all tables…",
  group: "open",
  order: 31,
  when: ({ connection }) => !keepsDataIn(connection),
  run: ({ connection, database, schema }) => openExport(connection, database, schema, null),
});

registerAction({
  id: "schema.paste_table",
  scope: "navigator.schema",
  title: "Paste CSV as a new table…",
  group: "open",
  order: 32,
  when: ({ connection }) => !isNoSql(connection.driver) && !takesNoRows(connection),
  run: async ({ connection, database, schema }) => {
    let path: string;
    try {
      path = await answerFor("clipboard", () => api.clipboardToFile());
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
  when: ({ connection, schema, selection }) =>
    !isNoSql(connection.driver) &&
    !keepsDataIn(connection) &&
    (selection?.length ?? 0) > 1 &&
    selection!.every((s) => s.object.kind === "table" && s.schema.name === schema.name),
  run: ({ connection, database, schema, selection }) =>
    openMigrate(connection, database, schema.name, selection!.map((s) => s.object.name)),
});

registerAction({
  id: "schema.migrate",
  scope: "navigator.schema",
  title: "Copy schema to a database…",
  group: "open",
  order: 33,
  // A schema copy reads the tables' definitions, which a collection has not.
  when: ({ connection }) => !isNoSql(connection.driver) && !keepsDataIn(connection),
  run: ({ connection, database, schema }) => openMigrate(connection, database, schema.name, null),
});

registerAction({
  id: "object.generate_data",
  scope: "navigator.object",
  title: "Generate data…",
  group: "manage",
  order: 74,
  // Fake keys and documents are not made: a Redis table is a key type, an index holds documents.
  when: ({ connection, object }) => object.kind === "table" && !isRedis(connection.driver) && !isSearch(connection.driver) && !takesNoRows(connection),
  run: ({ connection, database, schema, object }) =>
    workbench().openDialog((close) =>
      createElement(GenerateDataDialog, {
        connection,
        database,
        schema: schema.name,
        currentSchema: omitsSchema(connection.driver, schema.current),
        table: object.name,
        onClose: close,
      }),
    ),
});

function openRunFile(connection: SavedConnection, database: string | null) {
  workbench().openDialog((close) => createElement(RunSqlFileDialog, { connection, database, onClose: close }));
}

registerAction({
  id: "connection.run_sql_file",
  scope: "navigator.connection",
  title: "Run SQL file…",
  group: "open",
  order: 12,
  when: ({ connection }) => !isNoSql(connection.driver) && !connection.read_only,
  run: ({ connection }) => openRunFile(connection, null),
});

registerAction({
  id: "database.run_sql_file",
  scope: "navigator.database",
  title: "Run SQL file…",
  group: "open",
  order: 12,
  when: ({ connection }) => !isNoSql(connection.driver) && !connection.read_only,
  run: ({ connection, database }) => openRunFile(connection, database),
});

/** The kinds of object a dump writes. */
const DUMPED = new Set(["table", "sequence", "view", "materialized_view", "function", "procedure"]);

function openDump(connection: SavedConnection, database: string | null, schema: string, objects: string[] | null) {
  workbench().openDialog((close) => createElement(DumpSqlDialog, { connection, database, schema, objects, onClose: close }));
}

registerAction({
  id: "schema.dump_sql",
  scope: "navigator.schema",
  title: "Dump to SQL file…",
  group: "open",
  order: 34,
  when: ({ connection }) => !isNoSql(connection.driver) && !keepsDataIn(connection),
  run: ({ connection, database, schema }) => openDump(connection, database, schema.name, null),
});

registerAction({
  id: "object.dump_sql",
  scope: "navigator.object",
  title: "Dump to SQL file…",
  group: "manage",
  order: 75,
  multiple: true,
  // The objects of one schema, which the dump orders by their references.
  when: ({ connection, schema, object, selection }) =>
    !isNoSql(connection.driver) &&
    !keepsDataIn(connection) &&
    (selection ?? [{ schema, object }]).every((s) => s.schema.name === schema.name && DUMPED.has(s.object.kind)),
  run: ({ connection, database, schema, object, selection }) =>
    openDump(connection, database, schema.name, (selection ?? [{ object }]).map((s) => s.object.name)),
});

const openProfiles = (connectionId?: string) =>
  workbench().openDialog((close) => createElement(ProfilesDialog, { connectionId, onClose: close }));

registerAction({
  id: "app.transfer_profiles",
  scope: "global",
  title: "Transfer profiles…",
  run: () => openProfiles(),
});

registerAction({
  id: "connection.transfer_profiles",
  scope: "navigator.connection",
  title: "Transfer profiles…",
  group: "open",
  order: 29,
  run: ({ connection }) => openProfiles(connection.id),
});
