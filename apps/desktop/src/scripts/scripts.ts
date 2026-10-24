/**
 * Saved scripts of the active project: `.sql` files in the project's scripts folder, with
 * subfolders and the connection each one opens on. The panel lists them; the actions in
 * `actions.ts` change them and tell the panel to reload with `scriptsChanged`.
 */
import { api, type SavedConnection, type ScriptEntry } from "../api";
import { workbench } from "../workbench";

const listeners = new Set<() => void>();

export function onScriptsChanged(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function scriptsChanged() {
  listeners.forEach((l) => l());
}

/** The file of a script: `root` with the OS's separators, then the relative path. */
export function scriptFile(root: string, path: string): string {
  const sep = root.includes("\\") ? "\\" : "/";
  return `${root}${sep}${path.replaceAll("/", sep)}`;
}

/** The folder part of a relative path, `""` at the top. */
export function parentOf(path: string): string {
  return path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
}

export function nameOf(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** Opens a script in a SQL editor on `connection`. */
export async function openScript(root: string, entry: ScriptEntry, connection: SavedConnection) {
  const filePath = scriptFile(root, entry.path);
  const sql = await api.readTextFile(filePath);
  workbench().openTab("query", connection.id, { sql, filePath });
}

/** The connection a script opens on: its link if that connection is listed, else the first. */
export function connectionFor(entry: ScriptEntry, connections: SavedConnection[]): SavedConnection | undefined {
  return connections.find((c) => c.id === entry.connection) ?? connections[0];
}
