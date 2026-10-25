/**
 * Folders of connections inside a project: the tree the navigator shows, and the actions
 * of a folder's menu and of a connection's "Move to folder".
 */
import { registerAction } from "../actions";
import { api, type ConnectionFolder, type Project, type SavedConnection } from "../api";
import { askName } from "../ui/NameDialog";
import { workbench } from "../workbench";
import { t } from "../i18n";

declare module "../actions" {
  interface ActionScopes {
    /** A folder of connections in the navigator. */
    "navigator.folder": { project: Project; folder: ConnectionFolder };
  }
}

export interface FolderNode {
  folder: ConnectionFolder;
  children: FolderNode[];
  connections: SavedConnection[];
}

const byName = (a: FolderNode, b: FolderNode) => a.folder.name.localeCompare(b.folder.name);

/** The project's folders as a tree, by name, each with its connections in the project's
 *  order; `loose` are the connections at the root. */
export function folderTree(project: Project, connections: SavedConnection[]): { folders: FolderNode[]; loose: SavedConnection[] } {
  const nodes = new Map<string, FolderNode>((project.folders ?? []).map((f) => [f.id, { folder: f, children: [], connections: [] }]));
  const folders: FolderNode[] = [];
  for (const node of nodes.values()) {
    const parent = node.folder.parent ? nodes.get(node.folder.parent) : undefined;
    (parent ? parent.children : folders).push(node);
  }
  const loose: SavedConnection[] = [];
  for (const c of connections) {
    const folder = project.placement?.[c.id];
    const node = folder ? nodes.get(folder) : undefined;
    if (node) node.connections.push(c);
    else loose.push(c);
  }
  for (const node of nodes.values()) node.children.sort(byName);
  return { folders: folders.sort(byName), loose };
}

/** Every folder of the project with its path, for menus: `Prod › Replica`. */
export function folderPaths(project: Project): { id: string; label: string }[] {
  const out: { id: string; label: string }[] = [];
  const walk = (nodes: FolderNode[], prefix: string) => {
    for (const n of nodes) {
      const label = prefix ? `${prefix} › ${n.folder.name}` : n.folder.name;
      out.push({ id: n.folder.id, label });
      walk(n.children, label);
    }
  };
  walk(folderTree(project, []).folders, "");
  return out;
}

const save = async (change: Promise<Project[]>) => workbench().setProjects(await change);

/** Asks for a name and makes a folder in the project, inside `parent` or at the root. */
export function newFolder(project: Project, parent: string | null) {
  askName(t(parent ? "New subfolder" : "New folder"), t("New folder"), (name) =>
    save(api.createConnectionFolder(project.id, name, parent)),
  );
}

registerAction({
  id: "folder.new_connection",
  scope: "navigator.folder",
  title: "New connection here",
  group: "new",
  order: 10,
  run: ({ folder }) => workbench().newConnection(folder.id),
});

registerAction({
  id: "folder.new_subfolder",
  scope: "navigator.folder",
  title: "New subfolder…",
  group: "new",
  order: 11,
  run: ({ project, folder }) => newFolder(project, folder.id),
});

registerAction({
  id: "folder.rename",
  scope: "navigator.folder",
  title: "Rename…",
  group: "manage",
  order: 20,
  run: ({ project, folder }) =>
    askName(t("Rename folder"), folder.name, (name) => save(api.renameConnectionFolder(project.id, folder.id, name))),
});

registerAction({
  id: "folder.move_to_root",
  scope: "navigator.folder",
  title: "Move to top level",
  group: "manage",
  order: 21,
  when: ({ folder }) => !!folder.parent,
  run: ({ project, folder }) => save(api.moveConnectionFolder(project.id, folder.id, null)),
});

registerAction({
  id: "folder.delete",
  scope: "navigator.folder",
  title: "Delete folder",
  group: "danger",
  order: 90,
  run: ({ project, folder }) => save(api.deleteConnectionFolder(project.id, folder.id)),
});

/** "Move to folder" on the connections of the active project, one entry per folder. */
export function registerMoveToFolder(project: Project | null): () => void {
  if (!project) return () => {};
  const placement = project.placement ?? {};
  const removers = [
    registerAction({
      id: "folder.move_connection.root",
      scope: "navigator.connection",
      title: "Top level",
      submenu: "Move to folder",
      group: "project",
      order: 85,
      when: ({ connection }) => !!placement[connection.id],
      run: ({ connection }) => save(api.placeConnection(project.id, connection.id, null)),
    }),
    registerAction({
      id: "folder.move_connection.new",
      scope: "navigator.connection",
      title: "New folder…",
      submenu: "Move to folder",
      group: "project",
      order: 85,
      run: ({ connection }) =>
        askName(t("New folder"), t("New folder"), async (name) => {
          const before = new Set((project.folders ?? []).map((f) => f.id));
          const all = await api.createConnectionFolder(project.id, name, null);
          workbench().setProjects(all);
          const made = all.find((p) => p.id === project.id)?.folders?.find((f) => !before.has(f.id));
          if (made) await save(api.placeConnection(project.id, connection.id, made.id));
        }),
    }),
    ...folderPaths(project).map(({ id, label }) =>
      registerAction({
        id: `folder.move_connection.${id}`,
        scope: "navigator.connection",
        title: label,
        submenu: "Move to folder",
        group: "project",
        order: 85,
        when: ({ connection }) => placement[connection.id] !== id,
        run: ({ connection }) => save(api.placeConnection(project.id, connection.id, id)),
      }),
    ),
  ];
  return () => removers.forEach((remove) => remove());
}
