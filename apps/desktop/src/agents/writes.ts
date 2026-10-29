/**
 * The changes agents ask to run, waiting for the user's answer in the app (the bridge's
 * `agent-write-request` events), and how the dialog shows them. No app imports: the unit
 * tests load it as it is.
 */

/** A change an agent asks to run (`platypus_mcp::WriteRequest`). */
export interface WriteRequest {
  id: string;
  /** The agent, as it names itself. */
  agent: string;
  /** The run of the agent's server: "allow for this session" holds for it. */
  session: string;
  connection: string;
  statement: string;
  /** The rows the planner expects it to change, where it tells. */
  estimated_rows: number | null;
  /** Its plan as an indented tree, where the database explains it. */
  plan: string | null;
}

/** The user's answer (`platypus_mcp::Verdict`). */
export type Verdict = { answer: "once" } | { answer: "session" } | { answer: "deny"; reason: string | null };

/** A request in the queue, with when the app got it. */
export interface Waiting {
  request: WriteRequest;
  /** `Date.now()` when it arrived. */
  since: number;
}

/** How long the user has to answer before the change is refused (`ANSWER_TIMEOUT`). */
export const ANSWER_MS = 300_000;

/** `queue` with `request` at its end, unless it is there already. */
export function enqueue(queue: readonly Waiting[], request: WriteRequest, now: number): Waiting[] {
  return queue.some((w) => w.request.id === request.id) ? [...queue] : [...queue, { request, since: now }];
}

/** `queue` without the request `id`. */
export function settle(queue: readonly Waiting[], id: string): Waiting[] {
  return queue.filter((w) => w.request.id !== id);
}

/** The time left to answer, as `m:ss`; `0:00` once it is over. */
export function timeLeft(since: number, now: number): string {
  const seconds = Math.max(0, Math.ceil((since + ANSWER_MS - now) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/** The estimated rows rounded as a planner's estimate reads: whole, at least one when it
 *  expects any; `null` when it does not tell. */
export function estimatedRows(rows: number | null): number | null {
  if (rows === null || !Number.isFinite(rows) || rows < 0) return null;
  return rows > 0 && rows < 1 ? 1 : Math.round(rows);
}

/** The verdict of a refusal, with its reason when one was typed. */
export function denial(reason: string): Verdict {
  const trimmed = reason.trim();
  return { answer: "deny", reason: trimmed ? trimmed : null };
}
