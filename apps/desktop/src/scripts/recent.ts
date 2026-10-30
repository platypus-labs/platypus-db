// The scripts opened or saved last, for the welcome page. Kept in this browser profile.

export interface RecentScript {
  /** The script file, absolute. */
  path: string;
  /** The connection it was opened or saved on. */
  connectionId: string;
  /** Unix epoch milliseconds. */
  at: number;
}

const KEY = "platypus.recent-scripts.v1";
/** At most this many are kept. */
export const RECENT_SCRIPTS = 6;

/** The recent scripts, newest first. */
export function recentScripts(): RecentScript[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) ?? "[]") as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (r: Partial<RecentScript>): r is RecentScript => typeof r?.path === "string" && typeof r.connectionId === "string" && typeof r.at === "number",
    );
  } catch {
    return [];
  }
}

/** Puts a script at the top of the list, once. */
export function rememberScript(path: string, connectionId: string) {
  try {
    const rest = recentScripts().filter((r) => r.path !== path);
    localStorage.setItem(KEY, JSON.stringify([{ path, connectionId, at: Date.now() }, ...rest].slice(0, RECENT_SCRIPTS)));
  } catch {
    // Storage can be unavailable or full; the list is a convenience.
  }
}
