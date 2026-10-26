/**
 * The active project: which connections the navigator lists and which tabs the tab bar
 * shows. `null` is All connections, a fixed entry that is not a saved project. Every tab
 * stays mounted whatever the project, so sessions and open transactions survive a switch.
 */
import type { Project, ProjectFolder, SavedConnection } from "../api";

/** Where the active project's id is kept between starts. */
const ACTIVE_KEY = "platypus.project.v1";

/** A project's key in maps and saved state: its id, or `all` for All connections. */
export function projectKey(id: string | null | undefined): string {
  return id ?? "all";
}

/** The connections a project lists, in its order; all of them for `null`, in the saved
 *  list's order. */
export function connectionsOf(project: Project | null, all: SavedConnection[]): SavedConnection[] {
  if (!project) return all;
  const byId = new Map(all.map((c) => [c.id, c]));
  return project.connections.flatMap((id) => byId.get(id) ?? []);
}

/** The project active at the last start, if it still exists. */
export function loadActiveProject(projects: Project[]): string | null {
  try {
    const id = localStorage.getItem(ACTIVE_KEY);
    return id && projects.some((p) => p.id === id) ? id : null;
  } catch {
    return null;
  }
}

export function saveActiveProject(id: string | null) {
  try {
    if (id) localStorage.setItem(ACTIVE_KEY, id);
    else localStorage.removeItem(ACTIVE_KEY);
  } catch {
    // Storage can be unavailable; the project is a convenience.
  }
}

export type RailItem = { kind: "project"; project: Project } | { kind: "folder"; folder: ProjectFolder; projects: Project[] };

/** What the rail shows, in order: a folder stands where its first project is, and holds its
 *  projects in the folder's order. */
export function railItems(projects: Project[], folders: ProjectFolder[]): RailItem[] {
  const folderOf = new Map(folders.flatMap((f) => f.projects.map((id) => [id, f] as const)));
  const byId = new Map(projects.map((p) => [p.id, p]));
  const shown = new Set<string>();
  const out: RailItem[] = [];
  for (const project of projects) {
    const folder = folderOf.get(project.id);
    if (!folder) out.push({ kind: "project", project });
    else if (!shown.has(folder.id)) {
      shown.add(folder.id);
      out.push({ kind: "folder", folder, projects: folder.projects.flatMap((id) => byId.get(id) ?? []) });
    }
  }
  return out;
}

/** The projects in the order the rail shows them. */
export function railOrder(projects: Project[], folders: ProjectFolder[]): Project[] {
  return railItems(projects, folders).flatMap((i) => (i.kind === "project" ? [i.project] : i.projects));
}
