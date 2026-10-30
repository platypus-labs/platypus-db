// Which connections the compatibility check of a driver version tries. Types only, so the
// unit tests load this module without the app's.
import type { DriverKind, SavedConnection } from "../api";
import { testable } from "../connectionTest.ts";

/** How the probe of a connection went. */
export type ProbeOutcome =
  | { state: "queued" }
  | { state: "running" }
  | { state: "done"; summary: string; ms: number }
  | { state: "failed"; message: string }
  | { state: "skipped" };

/** The saved connections that use the downloaded driver of `driver`, split into those the
 *  check connects to by itself and those that ask for a password each time, which it leaves
 *  alone. */
export function compatTargets(connections: SavedConnection[], driver: DriverKind) {
  const using = connections.filter((c) => c.driver === driver);
  return { run: using.filter(testable), skipped: using.filter((c) => !testable(c)) };
}

/** Counts of a finished check, for its summary and to decide whether to offer a way back. */
export function compatTally(outcomes: Iterable<ProbeOutcome>) {
  const counts = { done: 0, failed: 0, skipped: 0 };
  for (const o of outcomes) if (o.state === "done" || o.state === "failed" || o.state === "skipped") counts[o.state]++;
  return counts;
}

/** The registry's versions (newest first) followed by those only the disk has, e.g. of a
 *  driver the registry dropped. */
export function mergeVersions(available: string[], installed: string[]): string[] {
  const out = [...available];
  for (const v of installed) if (!out.includes(v)) out.push(v);
  return out;
}
