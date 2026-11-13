// Finding the saved connection a shared query belongs to, and starting a new one from the
// descriptor when there is none. Pure, like `shareQuery.ts`.
import type { ShareTarget } from "./shareQuery";

/** The fields of a saved connection the descriptor is made of or compared with. */
interface TargetFields {
  name: string;
  driver: string;
  host: string;
  port: number | null;
  database: string;
  init?: { schema: string | null } | null;
}

/** The descriptor of `connection`, for the database the tab uses (`database`, else the saved
 *  one). Nothing secret is read: user, passwords, keys and tokens stay out by construction. */
export function describeTarget(connection: TargetFields, database?: string | null): ShareTarget {
  return {
    driver: connection.driver,
    host: connection.host,
    port: connection.port,
    database: database || connection.database,
    schema: connection.init?.schema || null,
    name: connection.name,
  };
}

/** Hosts ignore case; a port left empty and 0 both mean the driver's default. */
const sameHost = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();
const samePort = (a: number | null, b: number | null) => (a || 0) === (b || 0);
// Database names are case sensitive on most servers, and a file path on Windows is not: be
// lenient, since a wrong "exact" match is only a connection offered first, never run.
const sameDatabase = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/** Saved connections that fit a descriptor. */
interface TargetMatches<C> {
  /** Same driver, host, port and database. */
  exact: C[];
  /** Same driver but not the same place, the likeliest first: the same database, then the
   *  same host, then the connection with the sender's name. */
  candidates: C[];
}

/** Sorts `connections` into those that are the descriptor's target and those that may be. */
export function matchTarget<C extends TargetFields>(target: ShareTarget, connections: readonly C[]): TargetMatches<C> {
  const exact: C[] = [];
  const scored: [number, C][] = [];
  for (const c of connections) {
    if (c.driver !== target.driver) continue;
    const host = sameHost(c.host, target.host);
    const database = sameDatabase(c.database, target.database);
    if (host && samePort(c.port, target.port) && database) exact.push(c);
    else scored.push([(database ? 4 : 0) + (host ? 2 : 0) + (target.name && c.name === target.name ? 1 : 0), c]);
  }
  // Array.sort is stable: the saved order decides between equal scores.
  scored.sort((a, b) => b[0] - a[0]);
  return { exact, candidates: scored.map(([, c]) => c) };
}

/** `text` with each `${` that would read an environment variable written `$${`, which a
 *  connection reads as the text itself (as `held_references` in platypus-sync does). */
export function heldReferences(text: string): string {
  // A function's result is taken as it is: no `$` patterns in it.
  return text.replace(/(?<!\$)\$\{/g, () => "$${");
}

/** `base` (a new connection of the descriptor's driver) filled in from the descriptor. The
 *  sender's name is kept, with nothing else of theirs: no user or password is set, the
 *  receiver types their own. A `${env:NAME}` in the sender's fields is held (`$${env:NAME}`):
 *  it would read this computer's environment and could send a variable to the sender's
 *  server, so it is read only once the receiver edits it. */
export function prefillFromTarget<C extends TargetFields>(base: C, target: ShareTarget): C {
  return {
    ...base,
    name: target.name || base.name,
    driver: target.driver,
    host: target.host ? heldReferences(target.host) : base.host,
    port: target.port ?? base.port,
    database: heldReferences(target.database),
    // The default schema is part of what the query assumes.
    ...(target.schema ? { init: { time_zone: null, statements: "", ...base.init, schema: heldReferences(target.schema) } } : {}),
  } as C;
}
