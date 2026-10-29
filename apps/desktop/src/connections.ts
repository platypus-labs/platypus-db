/** Connection types (development, test, production) and what they change in the UI. */
import { type ConnectionType, isEmbedded, type ParseNote, type SavedConnection } from "./api";
import { msg, t } from "./i18n";
import { signInOf } from "./signIn";

export const CONNECTION_TYPES: { value: ConnectionType; label: string; color: string }[] = [
  { value: "dev", label: msg("Development"), color: "#2f9e44" },
  { value: "test", label: msg("Test"), color: "#f08c00" },
  { value: "prod", label: msg("Production"), color: "#c92a2a" },
];

/** The connection's color: its own, else its type's. */
export function connectionColor(c: SavedConnection | undefined): string | null {
  if (!c) return null;
  return c.color ?? CONNECTION_TYPES.find((t) => t.value === c.connection_type)?.color ?? null;
}

/** The password is asked at each connection: none is saved, looked up in `.pgpass`,
 *  printed by a command, or replaced by a token or Windows' sign-in. */
export const asksPassword = (c: SavedConnection) =>
  !isEmbedded(c.driver) && !c.save_password && !c.pgpass && !c.password_command && !signInOf(c);

/** What an imported or pasted connection needs before it can connect, or left out. */
export function noteText(note: ParseNote): string {
  switch (note.kind) {
    case "windows_auth":
      return t("Signs in with Windows: only PlatypusDB on Windows can connect this way.");
    case "entra_auth":
      return t("Signs in with Microsoft Entra ID: the Azure CLI (az) must be installed and signed in.");
    case "named_instance":
      return t("Named instance {instance}: set the port it listens on.", { instance: note.instance });
    case "missing_file":
      return t("The database file {path} was not found: choose it before connecting.", { path: note.path });
    case "ignored":
      return t("Not used here: {keys}.", { keys: note.keys.join(", ") });
    case "other_hosts":
      return t("Only the first host is used; left out: {hosts}.", { hosts: note.hosts.join(", ") });
  }
}

export const isProduction = (c: SavedConnection | undefined) => c?.connection_type === "prod";

/** A short label for badges: DEV, TEST, PROD. */
export function typeBadge(c: SavedConnection): string | null {
  return c.connection_type ? c.connection_type.toUpperCase() : null;
}
