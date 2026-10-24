// Switching projects from the keyboard (Ctrl+1 is All connections, Ctrl+2… the projects in
// rail order), and the menu of a project's square.
import { registerAction } from "../actions";
import type { Project } from "../api";
import { workbench } from "../workbench";

declare module "../actions" {
  interface ActionScopes {
    /** A project's square in the rail. */
    project: Project;
  }
}

/** All connections, then the projects, as the rail shows them. */
const order = () => [null, ...workbench().projects().map((p) => p.id)];

function step(delta: number) {
  const all = order();
  const at = all.indexOf(workbench().activeProject()?.id ?? null);
  workbench().switchProject(all[(at + delta + all.length) % all.length] ?? null);
}

for (let n = 1; n <= 9; n++) {
  registerAction({
    id: `project.switch.${n}`,
    scope: "global",
    title: n === 1 ? "Switch to All connections" : `Switch to project ${n - 1}`,
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
  id: "project.delete",
  scope: "project",
  title: "Delete project",
  group: "danger",
  order: 20,
  run: (project) => workbench().deleteProject(project),
});
