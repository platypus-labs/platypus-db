// Opens a saved transfer profile in the dialog it was saved from.
import { createElement } from "react";
import { api, type ProfileTask, type SavedConnection, toCommandError, type TransferProfile } from "../api";
import { ExportDialog } from "../components/ExportDialog";
import { basename } from "../workspace";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { ExportTablesDialog } from "./ExportTablesDialog";
import { ImportDataDialog, type TableAt } from "./ImportDataDialog";
import { MigrateDialog } from "./MigrateDialog";
import { missingConnections, profileConnections } from "./profiles";
import { runTracked } from "./ProfileSave";

type Plan = Extract<ProfileTask, { kind: "import_file" | "transfer_table" }>["plan"];

/** The table the plan's schema and name point at, in `connection`. */
const at = (connection: SavedConnection, database: string | null, schema: string | null, table: string | null): TableAt => ({
  connection,
  database,
  schema: schema ?? "",
  currentSchema: schema === null,
  table,
});

/** Opens `profile` in its dialog, with what it kept. Returns why it cannot be opened (a
 *  connection of it is gone), or `null`. */
export function openProfile(profile: TransferProfile): string | null {
  const connections = workbench().connections();
  const gone = missingConnections(profile.task, connections.map((c) => c.id));
  if (gone.length > 0) return t("A connection of the profile {name} is gone.", { name: profile.name });
  const [first, second] = profileConnections(profile.task).map((id) => connections.find((c) => c.id === id)!);
  const task = profile.task;
  workbench().openDialog((close) => {
    switch (task.kind) {
      case "export_query":
        return createElement(ExportDialog, {
          name: profile.name,
          driver: first!.driver,
          loadedRows: 0,
          complete: false,
          transactionOpen: false,
          queryOnly: true,
          profile,
          query: { connection_id: task.connection_id, database: task.database, sql: task.sql },
          onExport: (path, options) => {
            close();
            void runTracked(t("Export {name}", { name: basename(path) }), async (onEvent) => {
              const sid = await workbench().sideSession(first!, task.database);
              if (sid === null) throw new Error(t("Cancelled"));
              return api.exportQuery(sid, task.sql, path, options, onEvent);
            }).catch((e: unknown) => workbench().showError(toCommandError(e).message));
          },
          onClose: close,
        });
      case "export_tables":
        return createElement(ExportTablesDialog, {
          connection: first!,
          database: task.database,
          schema: task.tables[0]?.schema ?? "",
          currentSchema: true,
          tables: task.tables,
          profile,
          onClose: close,
        });
      case "import_file": {
        const plan: Plan = task.plan;
        return createElement(ImportDataDialog, {
          target: at(first!, task.database, plan.schema, plan.create ? null : plan.table),
          profile,
          onClose: close,
        });
      }
      case "transfer_table":
        return createElement(ImportDataDialog, {
          source: { ...at(first!, task.source_database, task.plan.source_schema ?? null, task.plan.source_table), table: task.plan.source_table },
          target: at(second!, task.target_database, task.plan.schema, null),
          newTable: task.plan.table,
          profile,
          onClose: close,
        });
      case "migrate":
        return createElement(MigrateDialog, {
          connection: first!,
          database: task.source_database,
          schema: task.source_schema,
          tables: task.tables,
          objects: task.objects,
          profile,
          onClose: close,
        });
    }
  });
  return null;
}
