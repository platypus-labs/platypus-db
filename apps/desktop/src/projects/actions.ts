// Switching projects from the keyboard (Ctrl+1 is All connections, Ctrl+2… the projects in
// rail order), the menu of a project's square, and adding a connection to a project from
// the navigator.
import { registerAction } from "../actions";
import { api, type Project, type ProjectFolder, toCommandError } from "../api";
import { askName } from "../ui/NameDialog";
import { workbench } from "../workbench";
import { PROJECT_COLORS } from "./ProjectDialog";
import { testConnections } from "../components/TestConnectionsDialog";
import { refreshShared } from "../sync/background";
import { openSharedProject, shareProject } from "./shared";
import { railOrder } from "./state";
import { t, msg } from "../i18n";

declare module "../actions" {
  interface ActionScopes {
    /** A project's square in the rail. */
    project: Project;
    /** A folder of projects in the rail. */
    "project.folder": ProjectFolder;
  }
}

/** All connections, then the projects, as the rail shows them. */
const order = () => [null, ...railOrder(workbench().projects(), workbench().projectFolders()).map((p) => p.id)];

function step(delta: number) {
  const all = order();
  const at = all.indexOf(workbench().activeProject()?.id ?? null);
  workbench().switchProject(all[(at + delta + all.length) % all.length] ?? null);
}

/** Ctrl+1 to Ctrl+9; written out so the translation check sees each one. */
const SWITCH_TITLES = [
  msg("Switch to All connections"),
  msg("Switch to project 1"),
  msg("Switch to project 2"),
  msg("Switch to project 3"),
  msg("Switch to project 4"),
  msg("Switch to project 5"),
  msg("Switch to project 6"),
  msg("Switch to project 7"),
  msg("Switch to project 8"),
];

for (let n = 1; n <= 9; n++) {
  registerAction({
    id: `project.switch.${n}`,
    scope: "global",
    title: SWITCH_TITLES[n - 1]!,
    shortcut: `Mod+${n}`,
    hidden: n > 1,
    run: () => {
      const all = order();
      if (n - 1 < all.length) workbench().switchProject(all[n - 1] ?? null);
    },
  });
}

registerAction({
  id: "project.next",
  scope: "global",
  title: "Next project",
  shortcut: "Mod+Alt+PageDown",
  run: () => step(1),
});

registerAction({
  id: "project.previous",
  scope: "global",
  title: "Previous project",
  shortcut: "Mod+Alt+PageUp",
  run: () => step(-1),
});

registerAction({
  id: "project.new",
  scope: "global",
  title: "New project…",
  run: () => workbench().editProject(null),
});

registerAction({
  id: "project.rename",
  scope: "project",
  title: "Rename…",
  order: 10,
  run: (project) => workbench().editProject(project),
});

registerAction({
  id: "project.color",
  scope: "project",
  title: "Color…",
  order: 11,
  run: (project) => workbench().editProject(project),
});

registerAction({
  id: "app.test_connections",
  scope: "global",
  title: "Test the connections of the project",
  run: () => testConnections(workbench().activeProject()),
});

registerAction({
  id: "project.test_connections",
  scope: "project",
  title: "Test connections",
  order: 12,
  run: (project) => testConnections(project),
});

registerAction({
  id: "project.share",
  scope: "project",
  title: "Share project…",
  order: 13,
  when: (project) => !project.shared,
  run: (project) => shareProject(project),
});

registerAction({
  id: "project.refresh_shared",
  scope: "project",
  title: "Read the shared folder again",
  order: 13,
  when: (project) => project.shared !== undefined,
  run: () => {
    refreshShared().then(
      (notices) => {
        if (notices.length === 0) workbench().notify(t("The project is up to date with its shared folder."));
      },
      (e: unknown) => workbench().showError(toCommandError(e).message),
    );
  },
});

registerAction({
  id: "project.open_shared",
  scope: "global",
  title: "Open shared project…",
  run: () => void openSharedProject(),
});

registerAction({
  id: "project.export",
  scope: "project",
  title: "Export…",
  order: 14,
  run: (project) => workbench().exportConnections(project.connections),
});

registerAction({
  id: "project.delete",
  scope: "project",
  title: "Delete project",
  group: "danger",
  order: 20,
  run: (project) => workbench().deleteProject(project),
});

registerAction({
  id: "project.remove_connection",
  scope: "navigator.connection",
  title: "Remove from this project",
  group: "project",
  order: 85,
  when: () => workbench().activeProject() !== null,
  run: async ({ connection }) => {
    const project = workbench().activeProject();
    if (project) workbench().setProjects(await api.setProjectMembership(connection.id, project.id, false));
  },
});

/** "Add to project ▸": one entry per project the connection is not in. Registered again
 *  whenever the projects change; returns the function that removes the entries. */
export function registerAddToProject(projects: Project[]): () => void {
  const removers = projects.map((p) =>
    registerAction({
      id: `project.add_to.${p.id}`,
      scope: "navigator.connection",
      title: p.name,
      submenu: "Add to project",
      group: "project",
      order: 84,
      when: ({ connection }) => !p.connections.includes(connection.id),
      run: async ({ connection }) =>
        workbench().setProjects(await api.setProjectMembership(connection.id, p.id, true)),
    }),
  );
  return () => removers.forEach((remove) => remove());
}

const COLOR_NAMES = [msg("Purple"), msg("Amber"), msg("Blue"), msg("Violet"), msg("Green"), msg("Red"), msg("Teal"), msg("Gray")];

const saveFolder = async (folder: ProjectFolder) => workbench().setWorkspace(await api.saveProjectFolder(folder));

registerAction({
  id: "project_folder.rename",
  scope: "project.folder",
  title: "Rename folder…",
  order: 10,
  run: (folder) => askName(t("Rename folder"), folder.name, (name) => saveFolder({ ...folder, name })),
});

PROJECT_COLORS.forEach((color, i) =>
  registerAction({
    id: `project_folder.color.${i}`,
    scope: "project.folder",
    title: COLOR_NAMES[i] ?? color,
    submenu: "Color",
    order: 11,
    when: (folder) => folder.color !== color,
    run: (folder) => saveFolder({ ...folder, color }),
  }),
);

registerAction({
  id: "project_folder.ungroup",
  scope: "project.folder",
  title: "Ungroup",
  group: "danger",
  order: 20,
  run: async (folder) => workbench().setWorkspace(await api.ungroupProjects(folder.id)),
});
