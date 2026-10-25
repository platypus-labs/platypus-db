// The menu of a script or folder in the Scripts panel, and "New script" on a connection.
import { registerAction } from "../actions";
import { api, type SavedConnection, type ScriptEntry } from "../api";
import { workbench } from "../workbench";
import { ask } from "../ui/ask";
import { connectionFor, nameOf, openScript, parentOf, scriptFile, scriptsChanged } from "./scripts";

declare module "../actions" {
  interface ActionScopes {
    /** A script or folder of the Scripts panel. */
    script: {
      entry: ScriptEntry;
      /** The active project, `null` for All connections. */
      project: string | null;
      root: string;
      connections: SavedConnection[];
      /** Puts the entry's name in edit mode. */
      rename: (path: string) => void;
    };
  }
}

const isFile = ({ entry }: { entry: ScriptEntry }) => !entry.folder;

registerAction({
  id: "script.open",
  scope: "script",
  title: "Open",
  group: "open",
  order: 1,
  when: (a) => isFile(a) && a.connections.length > 0,
  run: ({ entry, root, connections }) => openScript(root, entry, connectionFor(entry, connections)!),
});

registerAction({
  id: "script.new",
  scope: "script",
  title: "New script here",
  group: "new",
  order: 10,
  when: ({ entry }) => entry.folder,
  run: async ({ entry, project, connections, rename }) => {
    const only = connections.length === 1 ? connections[0]!.id : null;
    const path = await api.createScript(project, entry.path, "Script", only);
    scriptsChanged();
    rename(path);
  },
});

registerAction({
  id: "script.new_folder",
  scope: "script",
  title: "New folder here",
  group: "new",
  order: 11,
  when: ({ entry }) => entry.folder,
  run: async ({ entry, project, rename }) => {
    const path = `${entry.path}/New folder`;
    await api.createScriptFolder(project, path);
    scriptsChanged();
    rename(path);
  },
});

registerAction({
  id: "script.rename",
  scope: "script",
  title: "Rename",
  shortcut: "F2",
  group: "edit",
  order: 20,
  run: ({ entry, rename }) => rename(entry.path),
});

registerAction({
  id: "script.unlink",
  scope: "script",
  title: "Unlink from its connection",
  group: "link",
  order: 31,
  when: ({ entry }) => isFile({ entry }) && entry.connection !== null,
  run: async ({ entry, project }) => {
    await api.linkScript(project, entry.path, null);
    scriptsChanged();
  },
});

registerAction({
  id: "script.reveal_path",
  scope: "script",
  title: "Copy file path",
  group: "edit",
  order: 22,
  run: ({ entry, root }) => navigator.clipboard.writeText(scriptFile(root, entry.path)),
});

registerAction({
  id: "script.delete",
  scope: "script",
  title: "Delete…",
  group: "danger",
  order: 90,
  run: async ({ entry, project }) => {
    const what = entry.folder ? `the folder ${nameOf(entry.path)} and every script in it` : nameOf(entry.path);
    if (!(await ask(`Delete ${what}? The file${entry.folder ? "s are" : " is"} removed from disk.`, { confirm: "Delete", danger: true }))) return;
    await api.deleteScript(project, entry.path);
    scriptsChanged();
  },
});

/** "Open on ▸" and "Link to ▸" entries, one per connection of the project; returns the
 *  function that removes them. */
export function registerScriptConnections(connections: SavedConnection[]): () => void {
  const removers = connections.flatMap((c) => [
    registerAction({
      id: `script.open_on.${c.id}`,
      scope: "script",
      title: c.name,
      submenu: "Open on",
      group: "open",
      order: 2,
      when: (a) => isFile(a) && connections.length > 1,
      run: ({ entry, root }) => openScript(root, entry, c),
    }),
    registerAction({
      id: `script.link.${c.id}`,
      scope: "script",
      title: c.name,
      submenu: "Link to connection",
      group: "link",
      order: 30,
      when: ({ entry }) => !entry.folder && entry.connection !== c.id,
      run: async ({ entry, project }) => {
        await api.linkScript(project, entry.path, c.id);
        scriptsChanged();
      },
    }),
  ]);
  return () => removers.forEach((remove) => remove());
}

registerAction({
  id: "script.new_for_connection",
  scope: "navigator.connection",
  title: "New saved script",
  group: "open",
  order: 11,
  run: async ({ connection }) => {
    const project = workbench().activeProject()?.id ?? null;
    const path = await api.createScript(project, "", connection.name, connection.id);
    const { root } = await api.listScripts(project);
    scriptsChanged();
    await openScript(root, { path, folder: false, connection: connection.id }, connection);
  },
});

/** Moves a script or folder into `folder` (`""` for the top), keeping its name. */
export async function moveScript(project: string | null, path: string, folder: string) {
  const to = folder ? `${folder}/${nameOf(path)}` : nameOf(path);
  if (to === path || parentOf(path) === folder || to.startsWith(`${path}/`)) return;
  await api.renameScript(project, path, to);
  scriptsChanged();
}
