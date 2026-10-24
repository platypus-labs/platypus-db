/** Open tabs of every project, restored on the next start. Sessions are not persisted. */

const KEY = "platypus.workspace.v2";
/** 0.2.0 and before: the tabs of the one workspace there was. */
const KEY_V1 = "platypus.workspace.v1";

/** A tab's kind, connection and project plus whatever its kind persists
 *  (`TabKind.persist`). Tabs saved before tab kinds existed have no `kind`: they are SQL
 *  editors; tabs saved before projects have no `project`: they are in All connections. */
export interface PersistedTab {
  kind?: string;
  connectionId: string;
  project?: string | null;
  [field: string]: unknown;
}

export interface Workspace {
  tabs: PersistedTab[];
  /** The active tab of each project, as an index into `tabs`, by `projectKey`. */
  active: Record<string, number>;
}

/** The saved workspace; one from 0.2.0 becomes the tabs of All connections. */
export function loadWorkspace(): Workspace | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Workspace;
      return Array.isArray(parsed.tabs) ? parsed : null;
    }
    const old = localStorage.getItem(KEY_V1);
    if (!old) return null;
    const parsed = JSON.parse(old) as { tabs: PersistedTab[]; active: number };
    if (!Array.isArray(parsed.tabs)) return null;
    return { tabs: parsed.tabs.map((t) => ({ ...t, project: null })), active: { all: parsed.active } };
  } catch {
    return null;
  }
}

let timer: ReturnType<typeof setTimeout> | undefined;

/** Debounced: typing in the editor calls this on every keystroke. */
export function saveWorkspace(workspace: Workspace) {
  clearTimeout(timer);
  timer = setTimeout(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify(workspace));
      // Only once the new format is written, so a crash in between loses nothing.
      localStorage.removeItem(KEY_V1);
    } catch {
      // Storage can be unavailable or full; the workspace is a convenience.
    }
  }, 400);
}

export function basename(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}
