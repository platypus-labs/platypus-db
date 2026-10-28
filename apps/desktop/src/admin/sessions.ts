import type { ServerSession } from "../api";

/** A session and those that wait for it, and so on down. */
export interface Chain {
  session: ServerSession;
  waiting: Chain[];
}

/** The waits between `sessions` as trees: at the root, the sessions others wait for and
 *  that wait for none (or that only wait in a cycle, which would have no root). A session
 *  waiting for several shows under each. */
export function blockingChains(sessions: ServerSession[]): Chain[] {
  const byId = new Map(sessions.map((s) => [s.id, s]));
  const waiters = new Map<string, ServerSession[]>();
  for (const s of sessions)
    for (const blocker of s.blocked_by) {
      if (!byId.has(blocker)) continue;
      waiters.set(blocker, [...(waiters.get(blocker) ?? []), s]);
    }
  const tree = (s: ServerSession, path: Set<string>): Chain => {
    const inner = new Set(path).add(s.id);
    return {
      session: s,
      waiting: (waiters.get(s.id) ?? []).filter((w) => !inner.has(w.id)).map((w) => tree(w, inner)),
    };
  };
  const blocking = sessions.filter((s) => waiters.has(s.id));
  const roots = blocking.filter((s) => !s.blocked_by.some((b) => byId.has(b)));
  // Sessions in a cycle wait for each other: one of them stands for the cycle.
  const reached = new Set<string>();
  const mark = (c: Chain) => {
    reached.add(c.session.id);
    c.waiting.forEach(mark);
  };
  const chains = roots.map((s) => tree(s, new Set()));
  chains.forEach(mark);
  for (const s of blocking)
    if (!reached.has(s.id)) {
      const cycle = tree(s, new Set());
      mark(cycle);
      chains.push(cycle);
    }
  return chains;
}

/** Whether `filter` is in one of the fields of `session`, ignoring case. */
export function matches(session: ServerSession, filter: string): boolean {
  const needle = filter.trim().toLowerCase();
  if (!needle) return true;
  return [session.id, session.user, session.database, session.client, session.application, session.state, session.query]
    .some((field) => field?.toLowerCase().includes(needle));
}

/** `12.3 s`, `4 min 05 s`, `2 h 03 min`; empty for `null`. */
export function seconds(value: number | null): string {
  if (value === null) return "";
  if (value < 60) return `${value.toFixed(value < 10 ? 1 : 0)} s`;
  const whole = Math.floor(value);
  if (whole < 3600) return `${Math.floor(whole / 60)} min ${String(whole % 60).padStart(2, "0")} s`;
  return `${Math.floor(whole / 3600)} h ${String(Math.floor((whole % 3600) / 60)).padStart(2, "0")} min`;
}
