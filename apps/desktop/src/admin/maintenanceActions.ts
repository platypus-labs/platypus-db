// "Maintenance" in the navigator's menu of a table: the operations of its database, each
// asked first in a dialog that then shows what the server said.
import { createElement } from "react";
import { registerAction } from "../actions";
import type { DriverKind, Maintenance } from "../api";
import { msg } from "../i18n";
import { workbench } from "../workbench";
import { MaintenanceDialog } from "./MaintenanceDialog";

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
