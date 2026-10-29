import { createElement } from "react";
import { registerAction } from "../actions";
import { type DriverKind, isNoSql, type SavedConnection } from "../api";
import { msg } from "../i18n";
import { workbench } from "../workbench";
import { CompareDialog } from "./CompareDialog";
import { LintDialog } from "./LintDialog";
import { type ObjectFormKind, ObjectFormDialog } from "./ObjectFormDialog";
import { MigrationsDialog } from "./MigrationsDialog";

registerAction({
  id: "schema.lint",
  scope: "navigator.schema",
  title: "Check schema…",
  group: "open",
  order: 36,
  when: ({ connection }) => !isNoSql(connection.driver),
  run: ({ connection, database, schema }) =>
    workbench().openDialog((close) => createElement(LintDialog, { connection, database, schema: schema.name, onClose: close })),
});

registerAction({
  id: "schema.compare",
  scope: "navigator.schema",
  title: "Compare with…",
  group: "open",
  order: 37,
  when: ({ connection }) => !isNoSql(connection.driver),
  run: ({ connection, database, schema }) =>
    workbench().openDialog((close) => createElement(CompareDialog, { connection, database, schema: schema.name, onClose: close })),
});

/** The drivers whose dialect `platypus_schema::create_object` writes (SQLite's includes libSQL). */
const CREATABLE: DriverKind[] = ["postgres", "mysql", "sqlite", "libsql", "duckdb", "sqlserver", "oracle"];
/** MySQL is listed for sequences too: only MariaDB has them, which the dialog says. */
const WITH_SEQUENCES: DriverKind[] = ["postgres", "mysql", "duckdb", "sqlserver", "oracle"];
const NO_FUNCTIONS: DriverKind[] = ["sqlite", "libsql"];
const NO_TRIGGERS: DriverKind[] = ["duckdb"];

const openForm = (
  { connection, database, schemaName }: { connection: SavedConnection; database: string | null; schemaName: string },
  kind: ObjectFormKind,
  table?: string,
) =>
  workbench().openDialog((close) =>
    createElement(ObjectFormDialog, { connection, database, schema: schemaName, kind, table, onClose: close }),
  );

const CREATE: { kind: ObjectFormKind; title: string; order: number; allowed: (driver: DriverKind) => boolean }[] = [
  { kind: "view", title: msg("View…"), order: 38, allowed: () => true },
  { kind: "function", title: msg("Function…"), order: 39, allowed: (d) => !NO_FUNCTIONS.includes(d) },
  { kind: "trigger", title: msg("Trigger…"), order: 40, allowed: (d) => !NO_TRIGGERS.includes(d) },
  { kind: "sequence", title: msg("Sequence…"), order: 41, allowed: (d) => WITH_SEQUENCES.includes(d) },
];

for (const { kind, title, order, allowed } of CREATE) {
  registerAction({
    id: `schema.create_${kind}`,
    scope: "navigator.schema",
    title,
    submenu: "Create",
    group: "create",
    order,
    when: ({ connection }) => CREATABLE.includes(connection.driver) && allowed(connection.driver) && !connection.read_only,
    run: ({ connection, database, schema }) => openForm({ connection, database, schemaName: schema.name }, kind),
  });
}

registerAction({
  id: "object.create_trigger",
  scope: "navigator.object",
  title: "Create trigger…",
  group: "manage",
  order: 75,
  when: ({ connection, object }) =>
    object.kind === "table" && CREATABLE.includes(connection.driver) && !NO_TRIGGERS.includes(connection.driver) && !connection.read_only,
  run: ({ connection, database, schema, object }) =>
    openForm({ connection, database, schemaName: schema.name }, "trigger", object.name),
});

registerAction({
  id: "schema.migrations",
  scope: "navigator.schema",
  title: "Migration status…",
  group: "open",
  order: 38,
  when: ({ connection }) => !isNoSql(connection.driver),
  run: ({ connection, database, schema }) =>
    workbench().openDialog((close) => createElement(MigrationsDialog, { connection, database, schema: schema.name, onClose: close })),
});
