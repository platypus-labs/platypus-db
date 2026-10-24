/**
 * The active project: which connections the navigator lists and which tabs the tab bar
 * shows. `null` is All connections, a fixed entry that is not a saved project. Every tab
 * stays mounted whatever the project, so sessions and open transactions survive a switch.
 */
import type { Project, SavedConnection } from "../api";

/** Where the active project's id is kept between starts. */
const ACTIVE_KEY = "platypus.project.v1";

/** A project's key in maps and saved state: its id, or `all` for All connections. */
export function projectKey(id: string | null | undefined): string {
  return id ?? "all";
}

/** The connections a project lists, in the store's order; all of them for `null`. */
export function connectionsOf(project: Project | null, all: SavedConnection[]): SavedConnection[] {
  if (!project) return all;
  const members = new Set(project.connections);
  return all.filter((c) => members.has(c.id));
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
