/** What an edit of a saved connection means for the sessions already open on it. */
import type { DriverKind, SavedConnection } from "./api";

/** Fields that change nothing in an open session: the name and colour are shown, the
 *  read-only guard and the agents' access are checked by the app, virtual keys by the data
 *  editor, all from the saved connection as it is now. */
const UNUSED_BY_SESSIONS = new Set<string>(["name", "color", "read_only", "agent_access", "virtual_keys"]);

/** JSON with the keys of every object sorted, so that two values compare by content. */
function canonical(value: unknown): string {
  return JSON.stringify(value, (_key, v: unknown) =>
    v && typeof v === "object" && !Array.isArray(v)
      ? Object.fromEntries(Object.entries(v as Record<string, unknown>).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)))
      : v,
  );
}

/** Whether sessions opened before an edit no longer match the connection: another server,
 *  database, login, network path, init script, or type (production starts in manual
 *  commit). They are to be reopened. */
export function sessionSettingsChanged(before: SavedConnection, after: SavedConnection): boolean {
  const used = (c: SavedConnection) => Object.fromEntries(Object.entries(c).filter(([k, v]) => !UNUSED_BY_SESSIONS.has(k) && v !== undefined));
  return canonical(used(before)) !== canonical(used(after));
}

/** The databases whose sessions have a time zone the Init page can set; the others refuse
 *  one (see `SessionInit` in platypus-core). */
const TIME_ZONE_DRIVERS = new Set<DriverKind>(["postgres", "mysql", "clickhouse", "trino", "databricks", "snowflake", "oracle"]);

export const hasSessionTimeZone = (driver: DriverKind) => TIME_ZONE_DRIVERS.has(driver);

/** The connection without the Init settings its driver cannot apply, as a time zone left
 *  from another driver: hidden in the form, they would make every connection fail. */
export function withoutUnusableInit(c: SavedConnection): SavedConnection {
  if (!c.init?.time_zone || hasSessionTimeZone(c.driver)) return c;
  return { ...c, init: { ...c.init, time_zone: null } };
}
