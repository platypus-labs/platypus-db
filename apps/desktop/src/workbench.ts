/**
 * What features can ask of the app shell without being wired through props: open a tab,
 * show an error, get a connection's side session. `App` provides the implementation when
 * it mounts; actions and tab kinds call `workbench()`.
 */
import type { ReactNode } from "react";
import type { Capabilities, SavedConnection } from "./api";
import type { TabInit, TabKindName } from "./tabs/registry";

export interface Workbench {
  connections(): SavedConnection[];
  /** Opens a new tab of `kind` and makes it active. */
  openTab<K extends TabKindName>(kind: K, connectionId: string, init?: TabInit<K>): void;
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
