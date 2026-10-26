/**
 * What features can ask of the app shell without being wired through props: open a tab,
 * show an error, get a connection's side session. `App` provides the implementation when
 * it mounts; actions and tab kinds call `workbench()`.
 */
import type { ReactNode } from "react";
import type { Capabilities, Project, ProjectFolder, SavedConnection, Workspace } from "./api";
import type { TabInit, TabKindName, TabModel } from "./tabs/registry";

export interface Workbench {
  connections(): SavedConnection[];
  /** Replaces the saved connections, e.g. with what a command that changed one returned. */
  setConnections(all: SavedConnection[]): void;
  projects(): Project[];
  /** Replaces the projects with what a command that changed them returned. */
  setProjects(all: Project[]): void;
  /** The folders of the rail. */
  projectFolders(): ProjectFolder[];
  /** Replaces connections, projects and rail folders with what a command returned. */
  setWorkspace(workspace: Workspace): void;
  /** The project shown, `null` for All connections. */
  activeProject(): Project | null;
  switchProject(id: string | null): void;
  /** Opens the new connection dialog; the connection goes into `folder` of the active
   *  project when given. */
  newConnection(folder?: string | null): void;
  /** Opens the project dialog; `null` makes a new project. */
  editProject(project: Project | null): void;
  /** Deletes a project after asking, closing its tabs; its connections stay. */
  deleteProject(project: Project): Promise<void>;
  /** Opens a new tab of `kind` and makes it active. */
  openTab<K extends TabKindName>(kind: K, connectionId: string, init?: TabInit<K>): void;
  /** Opens a tab on a connection that is not saved, e.g. the in-memory DuckDB of "Query with
   *  SQL": it lives while one of its tabs is open, and `onClosed` runs once the last one closes.
   *  Its tabs are not kept for the next start. */
  openTemporaryTab<K extends TabKindName>(kind: K, connection: SavedConnection, init: TabInit<K>, onClosed: () => void): void;
  /** Makes active the first tab of the active project that `match` accepts; `false` if none. */
  focusTab(match: (tab: TabModel) => boolean): boolean;
  /** The navigator session of the connection (and database, `null` for the saved one),
   *  opened on first use; `null` if cancelled. */
  sideSession(connection: SavedConnection, database?: string | null): Promise<number | null>;
  /** What an open session's driver supports. */
  capabilities(sessionId: number): Capabilities | undefined;
  showError(message: string): void;
  openSettings(): void;
  /** Shows a dialog; `render` gets the function that closes it. */
  openDialog(render: (close: () => void) => ReactNode): void;
}

let current: Workbench | null = null;

export function provideWorkbench(workbench: Workbench) {
  current = workbench;
}

export function workbench(): Workbench {
  if (!current) throw new Error("the workbench is not ready yet");
  return current;
}

/** Sessions and catalogs are per connection and database; `null` is the saved one. */
export function dbKey(connectionId: string, database?: string | null) {
  return `${connectionId}\u0000${database ?? ""}`;
}

/** The connection and database of a `dbKey`. */
export function parseDbKey(key: string): { connectionId: string; database: string | null } {
  const [connectionId = "", database = ""] = key.split("\u0000");
  return { connectionId, database: database || null };
}
