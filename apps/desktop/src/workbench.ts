/**
 * What features can ask of the app shell without being wired through props: open a tab,
 * show an error, get a connection's side session. `App` provides the implementation when
 * it mounts; actions and tab kinds call `workbench()`.
 */
import type { SavedConnection } from "./api";
import type { TabInit, TabKindName } from "./tabs/registry";

export interface Workbench {
  connections(): SavedConnection[];
  /** Opens a new tab of `kind` and makes it active. */
  openTab<K extends TabKindName>(kind: K, connectionId: string, init?: TabInit<K>): void;
  /** The connection's navigator session, opened on first use; `null` if cancelled. */
  sideSession(connection: SavedConnection): Promise<number | null>;
  showError(message: string): void;
  openSettings(): void;
}

let current: Workbench | null = null;

export function provideWorkbench(workbench: Workbench) {
  current = workbench;
}

export function workbench(): Workbench {
  if (!current) throw new Error("the workbench is not ready yet");
  return current;
}
