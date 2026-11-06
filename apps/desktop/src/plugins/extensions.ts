// What plugins add beyond actions: the commands that open their tabs and panels, their commands on
// a grid's cell, and what plugs into the shell and the result grid. Loaded only once a plugin that
// is turned on has one of them (see `store.ts`): an app without any never reads this file.
import { registerAction } from "../actions";
import { api, type InstalledPlugin, toCommandError } from "../api";
import { MASK } from "../guards/maskRules";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { applyOutcome } from "./actions";
import { createGridView, typeMatches } from "./gridView";
import { PluginPanels, togglePanel } from "./PluginPanels";
import { type GridPluginInput, setPluginSlots } from "./slots";
import { installedPlugins, onPluginsChanged } from "./store";

/** The plugins whose grid failure the user has been told of, since the plugins last changed. */
const told = new Set<string>();

function attachGrid(input: GridPluginInput) {
  return createGridView(input, installedPlugins(), {
    mask: MASK,
    column: (plugin, extension, columns, rows) => api.pluginGridColumn(plugin, extension, columns, rows),
    render: (plugin, extension, column, values) => api.pluginGridRender(plugin, extension, column, values),
    failed: (plugin, title, reason) => {
      if (told.has(plugin.manifest.id)) return;
      told.add(plugin.manifest.id);
      workbench().showError(t("“{title}” of the plugin “{name}” failed: {reason}", { title, name: plugin.manifest.name, reason }));
    },
    reason: (error) => toCommandError(error).message,
  });
}

/** Opens a tab of the plugin on `connectionId`. */
function openTab(plugin: InstalledPlugin, kind: string, title: string, connectionId: string) {
  workbench().openTab("plugin", connectionId, { plugin: plugin.manifest.id, tabKind: kind, heading: title });
}

function registerTabs(plugin: InstalledPlugin): (() => void)[] {
  return (plugin.manifest.tabs ?? []).flatMap((tab) => {
    const common = { title: tab.title, group: "plugins", order: 900 };
    return [
      registerAction({
        ...common,
        id: `plugin.${plugin.manifest.id}.tab.${tab.kind}`,
        scope: "global",
        run: () => {
          const connection = workbench().currentConnection();
          if (connection) openTab(plugin, tab.kind, tab.title, connection);
          else workbench().notify(t("Add a connection first: a tab belongs to one."));
        },
      }),
      registerAction({
        ...common,
        id: `plugin.${plugin.manifest.id}.tab_on.${tab.kind}`,
        scope: "navigator.connection",
        submenu: "Plugins",
        run: ({ connection }) => openTab(plugin, tab.kind, tab.title, connection.id),
      }),
    ];
  });
}

function registerPanels(plugin: InstalledPlugin): (() => void)[] {
  return (plugin.manifest.panels ?? []).map((panel) =>
    registerAction({
      id: `plugin.${plugin.manifest.id}.panel.${panel.id}`,
      scope: "global",
      title: panel.title,
      group: "plugins",
      order: 900,
      run: () => togglePanel(plugin.manifest.id, panel.id),
    }),
  );
}

function registerGridActions(plugin: InstalledPlugin): (() => void)[] {
  return (plugin.manifest.grid?.actions ?? []).map((extension) =>
    registerAction({
      id: `plugin.${plugin.manifest.id}.grid.${extension.id}`,
      scope: "grid",
      title: extension.title,
      submenu: "Plugins",
      group: "plugins",
      order: 900,
      when: ({ grid, source }) => {
        const focused = grid.focused();
        const cell = focused && source?.pluginGrid?.cell(focused.row, focused.col);
        return !!cell && typeMatches(extension.types, cell.column.type);
      },
      run: async ({ grid, source }) => {
        const focused = grid.focused();
        const cell = focused && source?.pluginGrid?.cell(focused.row, focused.col);
        if (!cell) return;
        const connection = source?.connection;
        const outcome = await api.pluginGridAction(plugin.manifest.id, extension.id, cell.column, cell.value);
        await applyOutcome(outcome, connection ? { newTab: (sql) => workbench().openTab("query", connection.id, { sql }) } : {});
      },
    }),
  );
}

let registered: (() => void)[] = [];

function sync() {
  registered.forEach((remove) => remove());
  told.clear();
  const on = installedPlugins().filter((p) => p.enabled);
  const framed = on.filter((p) => p.approved.tabs);
  const griddy = on.filter((p) => p.approved.grid && p.manifest.grid);
  registered = [
    ...framed.flatMap(registerTabs),
    ...framed.flatMap(registerPanels),
    ...griddy.flatMap(registerGridActions),
  ];
  setPluginSlots({
    panels: framed.some((p) => p.manifest.panels?.length) ? PluginPanels : null,
    grid: griddy.length ? attachGrid : null,
  });
}

onPluginsChanged(sync);
sync();
