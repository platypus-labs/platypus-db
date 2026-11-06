// "Test connections": every saved connection of a project, or of All connections, tried a
// few at a time with the password it keeps, each with how it went.
// Types only: the unit tests load this module without the app's.
import type { SavedConnection, ServerInfo } from "./api";

/** How the test of a connection went. */
export type TestOutcome =
  | { state: "queued" }
  | { state: "running" }
  | { state: "done"; server: ServerInfo; ms: number }
  | { state: "failed"; message: string }
  | { state: "skipped" };

/** Whether `c` connects without asking: a file, a saved password, one from a command or
 *  from `.pgpass`, a token, Windows' sign-in or a Kerberos ticket. The others ask each time, and
 *  are not tried. */
export function testable(c: SavedConnection): boolean {
  // A file (`isEmbedded` in api.ts) needs no password. The ways of `SIGN_INS` in signIn.ts,
  // which the tests could not load from here.
  const signsIn = ["aws_iam", "gcp_iam", "entra_id", "windows", "kerberos"].includes(c.options?.auth ?? "");
  return c.driver === "sqlite" || c.driver === "duckdb" || c.save_password || !!c.password_command?.trim() || !!c.pgpass || signsIn;
}

/** The counts of a finished run, for its summary. */
export function tally(outcomes: Iterable<TestOutcome>) {
  const counts = { done: 0, failed: 0, skipped: 0 };
  for (const o of outcomes) if (o.state === "done" || o.state === "failed" || o.state === "skipped") counts[o.state]++;
  return counts;
}
