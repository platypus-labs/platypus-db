// The actions of the installed plugins in the action registry, under a "Plugins" submenu of
// the editor, the result grid and the navigator's objects. Each one sends what its place holds
// to the plugin (`plugin_run_action`) and applies the outcome the plugin answers: text for
// the editor, a new SQL tab, the clipboard or a message.
import { registerAction, type NavigatorObject } from "../actions";
import { api, type CommandError, type InstalledPlugin, type PluginActionInfo, type PluginOutcome } from "../api";
import type { GridActionContext } from "../grid/DataGrid";
import type { EditorView } from "@codemirror/view";
import type { EditorHost } from "../editor/SqlEditor";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { installedPlugins, onPluginsChanged, reloadPlugins } from "./store";

/** The grid rows sent to a plugin at most, as the backend caps them too. */
export const MAX_GRID_ROWS = 1000;

/** Where an outcome goes besides the clipboard and messages. */
interface Target {
  /** Puts the text in the editor. */
  replace?: (text: string) => void;
  /** Opens a new SQL tab with the text. */
  newTab?: (text: string) => void;
}

async function apply(outcome: PluginOutcome, target: Target) {
  switch (outcome.kind) {
    case "replace":
      if (!target.replace) throw failure(t("This plugin action can only change the text of an editor."));
      target.replace(outcome.text);
      break;
    case "new_tab":
      if (!target.newTab) throw failure(t("This plugin action cannot open a tab here."));
      target.newTab(outcome.text);
      break;
    case "copy":
      await navigator.clipboard.writeText(outcome.text);
      workbench().notify(t("Copied to the clipboard."));
      break;
    case "message":
      workbench().notify(outcome.text);
      break;
    case "none":
      break;
  }
}

/** An error the action's caller shows as it is. */
function failure(message: string): CommandError {
  return { kind: "plugin", message, code: null, detail: null, hint: null, position: null };
}

const actionId = (plugin: string, action: string) => `plugin.${plugin}.${action}`;

/** The editor's text and selection, and what the outcome does there. */
function editorContext(view: EditorView, driver: string, readOnly: boolean, host?: EditorHost) {
  const doc = view.state.doc.toString();
  const sel = view.state.selection.main;
  const context = {
    text: doc,
    selection: sel.empty ? null : { from: sel.from, to: sel.to, text: view.state.sliceDoc(sel.from, sel.to) },
    driver,
  };
  const target: Target = {
    replace: (text) => {
      if (readOnly) throw failure(t("The editor is read-only."));
      // Typing while the plugin ran wins.
      if (view.state.doc.toString() !== doc) return;
      const [from, to] = sel.empty ? [0, doc.length] : [sel.from, sel.to];
      view.dispatch({
        changes: { from, to, insert: text },
        selection: sel.empty ? undefined : { anchor: from, head: from + text.length },
        scrollIntoView: true,
        userEvent: "input.plugin",
      });
    },
    newTab: host?.openSql,
  };
  return { context, target };
}

/** The selected rows of a grid, at most `MAX_GRID_ROWS`, with their columns' names and
 *  types; values as "Copy as JSON" gives them. */
async function gridContext({ grid, source }: GridActionContext) {
  const sel = grid.selection();
  const jobId = source?.jobId;
  const last = sel ? grid.dataRow(sel.r1) : null;
  if (!sel || last === null || jobId == null) throw failure(t("Select some rows first."));
  const first = grid.dataRow(sel.r0) ?? 0;
  const count = last - first + 1;
  const limit = Math.min(count, MAX_GRID_ROWS);
  const all = grid.columns();
  const columns = sel.cols.map((c) => ({ name: all[c]!.name, type: all[c]!.db_type }));
  const text = await api.copyRows(jobId, first, limit, sel.cols, "json");
  const objects = JSON.parse(text) as Record<string, unknown>[];
  const rows = objects.map((o) => columns.map((c) => o[c.name] ?? null));
  const context = { columns, rows, truncated: count > limit };
  const connection = source?.connection;
  const target: Target = connection ? { newTab: (sql) => workbench().openTab("query", connection.id, { sql }) } : {};
  return { context, target };
}

/** The object and its connection, without anything secret. */
function objectContext(arg: NavigatorObject) {
  const { connection, database, schema, object, target: name } = arg;
  const context = {
    connection: {
      name: connection.name,
      driver: connection.driver,
      host: connection.host,
      port: connection.port,
      database: connection.database,
      user: connection.user,
    },
    database,
    schema: schema.name,
    object: { name: object.name, kind: object.kind },
    target: name,
  };
  const target: Target = { newTab: (sql) => workbench().openTab("query", connection.id, { sql, database }) };
  return { context, target };
}

async function run(plugin: InstalledPlugin, action: PluginActionInfo, build: () => Promise<{ context: unknown; target: Target }>) {
  const { context, target } = await build();
  const outcome = await api.pluginRunAction(plugin.manifest.id, action.id, context);
  await apply(outcome, target);
}

/** Adds the action to the registry; returns what removes it. */
function register(plugin: InstalledPlugin, action: PluginActionInfo): () => void {
  const common = {
    id: actionId(plugin.manifest.id, action.id),
    title: action.title,
    shortcut: action.shortcut ?? undefined,
    group: "plugins",
    order: 900,
  };
  switch (action.scope) {
    case "editor":
      return registerAction({
        ...common,
        scope: "editor",
        submenu: "Plugins",
        run: ({ view, driver, readOnly, host }) =>
          run(plugin, action, () => Promise.resolve(editorContext(view, driver, readOnly, host))),
      });
    case "grid":
      return registerAction({
        ...common,
        scope: "grid",
        submenu: "Plugins",
        when: (ctx) => ctx.source?.jobId != null,
        run: (ctx) => run(plugin, action, () => gridContext(ctx)),
      });
    case "navigator.object":
      return registerAction({
        ...common,
        scope: "navigator.object",
        submenu: "Plugins",
        run: (arg) => run(plugin, action, () => Promise.resolve(objectContext(arg))),
      });
  }
}

let registered: (() => void)[] = [];

/** Registers the actions of the plugins that are turned on, in place of the previous ones. */
function sync() {
  registered.forEach((remove) => remove());
  registered = installedPlugins()
    .filter((p) => p.enabled)
    .flatMap((p) => p.description.actions.map((a) => register(p, a)));
}

onPluginsChanged(sync);
reloadPlugins().catch((e: unknown) => console.warn("plugins", e));
