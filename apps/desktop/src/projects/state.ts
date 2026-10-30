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

/** Where in the rail a dragged project is dropped. `inside` a project makes a folder of the
 *  two (or joins its folder); inside a folder puts it last there; before or after a project
 *  in an open folder puts it there in the folder; `start` and `end` are the two ends of the
 *  rail, out of any folder. */
export type RailSpot =
  | { kind: "project" | "folder"; id: string; zone: "before" | "inside" | "after" }
  | { kind: "start" | "end" };

/** The store changes of a drop in the rail, made in this order: grouped with the project
 *  `group`, moved into (or out of, `id` `null`) a folder at `index`, then the projects put
 *  in `order` so the rail shows them where they were dropped. */
export interface RailChange {
  group?: string;
  folder?: { id: string | null; index: number | null };
  order: string[];
}

/** The change a drop of the project `dragged` on `spot` makes; `null` when it changes
 *  nothing. */
export function railDrop(projects: Project[], folders: ProjectFolder[], dragged: string, spot: RailSpot): RailChange | null {
  const flat = railOrder(projects, folders).map((p) => p.id);
  const folderOf = (id: string) => folders.find((f) => f.projects.includes(id)) ?? null;
  const from = folderOf(dragged);
  const others = flat.filter((id) => id !== dragged);
  /** The folder's projects other than the dragged one, in its order. */
  const members = (f: ProjectFolder) => f.projects.filter((id) => id !== dragged && others.includes(id));
  /** `others` with the dragged project at `at`. */
  const placed = (at: number) => [...others.slice(0, at), dragged, ...others.slice(at)];
  const after = (id: string | undefined) => (id === undefined ? others.length : others.indexOf(id) + 1);
  const out = from ? { folder: { id: null, index: null } } : {};

  let change: RailChange;
  switch (spot.kind) {
    case "start":
      change = { ...out, order: placed(0) };
      break;
    case "end":
      change = { ...out, order: placed(others.length) };
      break;
    case "project": {
      if (spot.id === dragged) return null;
      const target = folderOf(spot.id);
      if (spot.zone === "inside") {
        if (target && target === from) return null;
        const last = target ? members(target).at(-1) : spot.id;
        change = { group: spot.id, order: placed(after(last)) };
      } else {
        const at = others.indexOf(spot.id) + (spot.zone === "after" ? 1 : 0);
        if (target) {
          const index = members(target).indexOf(spot.id) + (spot.zone === "after" ? 1 : 0);
          change = { folder: { id: target.id, index }, order: placed(at) };
        } else {
          change = { ...out, order: placed(at) };
        }
      }
      break;
    }
    case "folder": {
      const folder = folders.find((f) => f.id === spot.id);
      if (!folder) return null;
      const inner = members(folder);
      if (spot.zone === "inside") {
        if (folder === from) return null;
        change = { folder: { id: folder.id, index: null }, order: placed(after(inner.at(-1))) };
      } else {
        // A folder of the dragged project alone: out of it, where it is.
        const at = inner.length === 0 ? flat.indexOf(dragged) : spot.zone === "before" ? others.indexOf(inner[0]!) : after(inner.at(-1));
        change = { ...out, order: placed(at) };
      }
      break;
    }
  }
  const moved = change.order.some((id, i) => id !== flat[i]);
  const target = change.group ? folderOf(change.group) : change.folder ? folders.find((f) => f.id === change.folder!.id) ?? null : from;
  return moved || target !== from || change.group ? change : null;
}
