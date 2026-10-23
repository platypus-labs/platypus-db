/** Open tabs, restored on the next start. Sessions are not persisted. */

const KEY = "platypus.workspace.v1";

/** A tab's kind and connection plus whatever its kind persists (`TabKind.persist`).
 *  Tabs saved before tab kinds existed have no `kind`: they are SQL editors. */
export interface PersistedTab {
  kind?: string;
  connectionId: string;
  [field: string]: unknown;
}

export interface Workspace {
  tabs: PersistedTab[];
  active: number;
}

export function loadWorkspace(): Workspace | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Workspace;
    return Array.isArray(parsed.tabs) ? parsed : null;
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
    } catch {
      // Storage can be unavailable or full; the workspace is a convenience.
    }
  }, 400);
}

export function basename(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}
