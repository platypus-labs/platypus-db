/** Open editor tabs, restored on the next start. Sessions are not persisted. */

const KEY = "platypus.workspace.v1";

export interface PersistedTab {
  connectionId: string;
  sql: string;
  /** Script file the tab was opened from or saved to. */
  filePath: string | null;
  /** File contents at the last open/save, to show unsaved changes. */
  savedSql: string | null;
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
