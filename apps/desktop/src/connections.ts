/** Connection types (development, test, production) and what they change in the UI. */
import type { ConnectionType, SavedConnection } from "./api";

export const CONNECTION_TYPES: { value: ConnectionType; label: string; color: string }[] = [
  { value: "dev", label: "Development", color: "#2f9e44" },
  { value: "test", label: "Test", color: "#f08c00" },
  { value: "prod", label: "Production", color: "#c92a2a" },
];

/** The connection's color: its own, else its type's. */
export function connectionColor(c: SavedConnection | undefined): string | null {
  if (!c) return null;
  return c.color ?? CONNECTION_TYPES.find((t) => t.value === c.connection_type)?.color ?? null;
}

export const isProduction = (c: SavedConnection | undefined) => c?.connection_type === "prod";

/** A short label for badges: DEV, TEST, PROD. */
export function typeBadge(c: SavedConnection): string | null {
  return c.connection_type ? c.connection_type.toUpperCase() : null;
}
