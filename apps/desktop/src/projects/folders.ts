/**
 * Folders of connections inside a project: the tree the navigator shows, the actions of a
 * folder's menu and of a connection's "Move to folder", and the moves of drag and drop and
 * Alt+Up/Alt+Down.
 */
import { registerAction } from "../actions";
import { api, type ConnectionFolder, type Project, type SavedConnection } from "../api";
import { askName } from "../ui/NameDialog";
import { workbench } from "../workbench";
import { type Arrange, type DragItem, listLayout, stepChange } from "./arrange";
import { t, msg } from "../i18n";

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

/** The project's folders as a tree in the project's order, each with its connections in
 *  the order of `connections`; `loose` are the connections at the root. */
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
  return { folders, loose };
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

/** Asks for a new name of a folder of the project. */
function renameFolder(project: Project, folder: ConnectionFolder) {
  askName(t("Rename folder"), folder.name, (name) => save(api.renameConnectionFolder(project.id, folder.id, name)));
}

/** Makes a change of `arrange.ts` in the store: in `project`, or (`null`) in the list All
 *  connections shows. A new folder of two connections is offered for renaming at once. */
export async function applyArrange(project: Project | null, change: Arrange) {
  if (!project) {
    if (change.kind === "connection") workbench().setWorkspace(await api.reorderConnection(change.id, change.before));
    return;
  }
  if (change.kind === "connection") {
    await save(api.arrangeConnection(project.id, change.id, change.folder, change.before));
  } else if (change.kind === "folder") {
    await save(api.arrangeConnectionFolder(project.id, change.id, change.parent, change.before));
  } else {
    const grouped = await api.groupConnections(project.id, change.target, change.dragged, t("New folder"));
    workbench().setProjects(grouped.projects);
    const updated = grouped.projects.find((p) => p.id === project.id);
    const made = updated?.folders?.find((f) => f.id === grouped.folder);
    if (updated && made) renameFolder(updated, made);
  }
}

/** The move of `item` one place up (`-1`) or down among its siblings, in the active project
 *  or in All connections; `null` at the edge. */
function stepOf(item: DragItem, delta: -1 | 1) {
  const project = workbench().activeProject();
  return stepChange(project ?? listLayout(workbench().connections()), item, delta);
}

function step(item: DragItem, delta: -1 | 1) {
  const change = stepOf(item, delta);
  return change ? applyArrange(workbench().activeProject(), change) : undefined;
}

const STEPS = [
  { delta: -1, name: "up", title: msg("Move up"), shortcut: "Alt+ArrowUp" },
  { delta: 1, name: "down", title: msg("Move down"), shortcut: "Alt+ArrowDown" },
] as const;

for (const { delta, name, title, shortcut } of STEPS) {
  registerAction({
    id: `connection.move_${name}`,
    scope: "navigator.connection",
    title,
    shortcut,
    group: "arrange",
    order: 86,
    when: ({ connection }) => stepOf({ kind: "connection", id: connection.id }, delta) !== null,
    run: ({ connection }) => step({ kind: "connection", id: connection.id }, delta),
  });
  registerAction({
    id: `folder.move_${name}`,
    scope: "navigator.folder",
    title,
    shortcut,
    group: "arrange",
    order: 22,
    when: ({ folder }) => stepOf({ kind: "folder", id: folder.id }, delta) !== null,
    run: ({ folder }) => step({ kind: "folder", id: folder.id }, delta),
  });
}

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
  run: ({ project, folder }) => renameFolder(project, folder),
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
