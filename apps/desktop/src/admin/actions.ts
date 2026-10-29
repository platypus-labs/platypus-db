// The navigator's administration actions: "Maintenance" in the menu of a table (the
// operations of its database, each asked first in a dialog that then shows what the server
// said), "Back up…" and "Restore…" of a database or schema, and the scheduled tasks.
import { createElement } from "react";
import { registerAction } from "../actions";
import type { DriverKind, Maintenance, SavedConnection } from "../api";
import { msg } from "../i18n";
import { workbench } from "../workbench";
import { BackupDialog, type BackupMode } from "./BackupDialog";
import { MaintenanceDialog } from "./MaintenanceDialog";
import { SchedulesDialog } from "./SchedulesDialog";

/** The operations of each database's driver (`Capabilities::maintenance`), known before a
 *  session is open. */
const OPERATIONS: Partial<Record<DriverKind, Maintenance[]>> = {
  postgres: ["vacuum", "analyze", "reindex"],
  mysql: ["analyze", "optimize", "check"],
  sqlserver: ["analyze", "reindex", "check"],
};

const TITLES: Record<Maintenance, string> = {
  vacuum: msg("Vacuum…"),
  analyze: msg("Analyze…"),
  reindex: msg("Reindex…"),
  optimize: msg("Optimize…"),
  check: msg("Check…"),
};

(Object.keys(TITLES) as Maintenance[]).forEach((operation, i) =>
  registerAction({
    id: `object.maintenance.${operation}`,
    scope: "navigator.object",
    title: TITLES[operation],
    submenu: "Maintenance",
    group: "manage",
    order: 70 + i,
    when: ({ connection, object }) =>
      object.kind === "table" && (OPERATIONS[connection.driver]?.includes(operation) ?? false),
    run: ({ connection, database, schema, object }) =>
      workbench().openDialog((close) =>
        createElement(MaintenanceDialog, {
          connection,
          database,
          schema: schema.name,
          table: object.name,
          operation,
          onClose: close,
        }),
      ),
  }),
);

/** The databases whose tools back up and restore (`commands::backup`). */
const backs = (connection: SavedConnection) => connection.driver === "postgres" || connection.driver === "mysql";

const openBackup = (mode: BackupMode, connection: SavedConnection, database: string | null, schema: string | null = null) =>
  workbench().openDialog((close) => createElement(BackupDialog, { mode, connection, database, schema, onClose: close }));

for (const [mode, title, order] of [
  ["backup", msg("Back up…"), 26],
  ["restore", msg("Restore…"), 27],
] as const) {
  registerAction({
    id: `connection.${mode}`,
    scope: "navigator.connection",
    title,
    group: "open",
    order,
    when: ({ connection }) => backs(connection),
    run: ({ connection }) => openBackup(mode, connection, null),
  });
  registerAction({
    id: `database.${mode}`,
    scope: "navigator.database",
    title,
    group: "open",
    order,
    when: ({ connection }) => backs(connection),
    run: ({ connection, database }) => openBackup(mode, connection, database),
  });
}

registerAction({
  id: "schema.backup",
  scope: "navigator.schema",
  title: "Back up schema…",
  group: "open",
  order: 26,
  // MySQL's schemas are databases: the connection's own is backed up from its menu.
  when: ({ connection, schema }) => connection.driver === "postgres" && !schema.system,
  run: ({ connection, database, schema }) => openBackup("backup", connection, database, schema.name),
});

const openSchedules = (connectionId?: string) =>
  workbench().openDialog((close) => createElement(SchedulesDialog, { connectionId, onClose: close }));

registerAction({
  id: "app.schedules",
  scope: "global",
  title: "Scheduled tasks…",
  run: () => openSchedules(),
});

registerAction({
  id: "connection.schedule",
  scope: "navigator.connection",
  title: "Schedule a task…",
  group: "open",
  order: 28,
  run: ({ connection }) => openSchedules(connection.id),
});
