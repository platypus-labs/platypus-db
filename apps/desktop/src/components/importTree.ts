/**
 * The import dialog's preview: each imported project as a tree of folders and connections,
 * what is ticked in it, and the request that saves the ticked part.
 */
import type { ImportedProject, ImportInto, ImportRequest, Project, SavedConnection, SkippedConnection } from "../api";

export interface FolderView {
  /** Names from the project's root; empty for the root itself. */
  path: string[];
  folders: FolderView[];
  connections: SavedConnection[];
}

const samePath = (a: string[], b: string[]) => a.length === b.length && a.every((n, i) => n === b[i]);

/** The project's folders as a tree, folders before connections, both in file order. */
export function projectTree(project: ImportedProject): FolderView {
  const root: FolderView = { path: [], folders: [], connections: [] };
  const find = (path: string[]): FolderView => {
    let at = root;
    for (let end = 1; end <= path.length; end++) {
      const sub = path.slice(0, end);
      let next = at.folders.find((f) => samePath(f.path, sub));
      if (!next) {
        next = { path: sub, folders: [], connections: [] };
        at.folders.push(next);
      }
      at = next;
    }
    return at;
  };
  for (const path of project.folders) find(path);
  for (const c of project.connections) find(project.placement[c.id] ?? []).connections.push(c);
  return root;
}

/** The connections in a folder and its subfolders. */
export function connectionsUnder(folder: FolderView): SavedConnection[] {
  return [...folder.connections, ...folder.folders.flatMap(connectionsUnder)];
}

export type Check = "on" | "off" | "mixed";

export function checkState(ids: string[], picked: ReadonlySet<string>): Check {
  const on = ids.filter((id) => picked.has(id)).length;
  return on === 0 ? "off" : on === ids.length ? "on" : "mixed";
}

/** Ticks all of `ids`, or unticks them when they are all ticked already. */
export function toggleAll(ids: string[], picked: ReadonlySet<string>): Set<string> {
  const next = new Set(picked);
  const on = checkState(ids, picked) === "on";
  for (const id of ids) {
    if (on) next.delete(id);
    else next.add(id);
  }
  return next;
}

/** A named project goes into the saved project of that name (an earlier import of the
 *  same file), else becomes a new project; a file of plain connections joins the active
 *  project, or no project in All connections. */
export function defaultInto(project: ImportedProject, active: string | null, existing: readonly Project[] = []): ImportInto {
  if (project.name) {
    const name = project.name.trim().toLowerCase();
    const same = existing.find((p) => p.name.trim().toLowerCase() === name);
    return same ? { kind: "existing", id: same.id } : { kind: "new_project", name: project.name };
  }
  return active ? { kind: "existing", id: active } : { kind: "none" };
}

/** The ticked part of each project. A folder comes along when it holds a ticked
 *  connection, or was empty in the file. */
function picked(project: ImportedProject, ticked: ReadonlySet<string>): ImportedProject {
  const connections = project.connections.filter((c) => ticked.has(c.id));
  const under = (path: string[]) =>
    project.connections.filter((c) => {
      const at = project.placement[c.id] ?? [];
      return at.length >= path.length && samePath(at.slice(0, path.length), path);
    });
  const folders = project.folders.filter((path) => {
    const inside = under(path);
    return inside.length === 0 || inside.some((c) => ticked.has(c.id));
  });
  const placement = Object.fromEntries(Object.entries(project.placement).filter(([id]) => ticked.has(id)));
  const with_password = (project.with_password ?? []).filter((id) => ticked.has(id));
  return { ...project, connections, folders, placement, skipped: [], notes: {}, project_notes: [], with_password };
}

/** How many connections of the file come with a saved password. */
export function savedPasswords(projects: ImportedProject[]): number {
  return projects.reduce((n, p) => n + (p.with_password?.length ?? 0), 0);
}

/** The saved connection `c` looks like: same database, server, port, database name and user. */
export function savedTwin(c: SavedConnection, existing: readonly SavedConnection[]): SavedConnection | undefined {
  return existing.find(
    (e) => e.driver === c.driver && e.host === c.host && e.port === c.port && e.database === c.database && e.user === c.user,
  );
}

/** The ticked connections that replace their saved twin, imported id → saved id. Each saved
 *  connection is replaced once, by the first that matches it; the others are added. */
export function replacements(
  projects: ImportedProject[],
  ticked: ReadonlySet<string>,
  existing: readonly SavedConnection[],
): Record<string, string> {
  const out: Record<string, string> = {};
  const taken = new Set<string>();
  for (const c of projects.flatMap((p) => p.connections)) {
    const twin = ticked.has(c.id) ? savedTwin(c, existing) : undefined;
    if (twin && !taken.has(twin.id)) {
      taken.add(twin.id);
      out[c.id] = twin.id;
    }
  }
  return out;
}

/** Whether the file held no connection at all, imported or left out. */
export function isEmptyImport(projects: ImportedProject[]): boolean {
  return projects.every((p) => p.connections.length === 0 && p.skipped.length === 0);
}

/** The tool's ids of a left-out connection's database and driver, e.g. `oracle · oracle_thin`. */
export function skippedIds(s: SkippedConnection): string {
  return [s.provider, s.driver].filter(Boolean).join(" · ") || "?";
}

export function importRequest(
  projects: ImportedProject[],
  ticked: ReadonlySet<string>,
  into: ImportInto[],
  railFolder: string | null,
  passwords = false,
  replace: Record<string, string> = {},
): ImportRequest {
  const targets = projects
    .map((p, i) => ({ project: picked(p, ticked), into: into[i] ?? { kind: "none" as const } }))
    .filter((t) => t.project.connections.length > 0);
  return { targets, rail_folder: railFolder?.trim() || null, passwords, replace };
}
