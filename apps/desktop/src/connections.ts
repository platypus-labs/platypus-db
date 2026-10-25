/** Connection types (development, test, production) and what they change in the UI. */
import { type ConnectionType, isEmbedded, type SavedConnection } from "./api";
import { msg } from "./i18n";

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

/** The password is asked at each connection: none is saved, looked up in `.pgpass` or
 *  printed by a command. */
export const asksPassword = (c: SavedConnection) =>
  !isEmbedded(c.driver) && !c.save_password && !c.pgpass && !c.password_command;

export const isProduction = (c: SavedConnection | undefined) => c?.connection_type === "prod";

/** A short label for badges: DEV, TEST, PROD. */
export function typeBadge(c: SavedConnection): string | null {
  return c.connection_type ? c.connection_type.toUpperCase() : null;
}
