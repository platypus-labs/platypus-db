/** Saved dashboards, the files in the data folder: opening one in a tab. */
import { api, type SavedConnection } from "../api";
import { workbench } from "../workbench";
import { parseConfig } from "../analysis/chartConfig";
import { parseDashboardFile } from "./model";

/** Opens the saved dashboard `name` of `project` in a tab, on the connection the file names
 *  (`fallback` when that one is gone); the tab it is already open in comes forward instead. */
export async function openSavedDashboard(project: string | null, name: string, fallback: SavedConnection) {
  const shown = workbench().focusTab((tab) => tab.kind === "dashboard" && tab.name === name && (tab.project ?? null) === project);
  if (shown) return;
  const { content } = parseDashboardFile(await api.readDashboard(project, name), parseConfig);
  const connection = workbench().connections().find((c) => c.id === content.connection) ?? fallback;
  workbench().openTab("dashboard", connection.id, { widgets: content.widgets, refresh: content.refresh, title: name, name });
}
