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

/** A refusal, with the reason when the user typed one. */
export interface Denial {
  answer: "deny";
  reason: string | null;
}

/** The user's answer (`platypus_mcp::Verdict`). */
export type Verdict = { answer: "once" } | { answer: "session" } | Denial;

/** A request in the queue, with when the app got it: a change, or (`context.ts`) a look at
 *  the user's work. */
export interface Waiting<R extends { id: string } = WriteRequest> {
  request: R;
  /** `Date.now()` when it arrived. */
  since: number;
}

/** How long the user has to answer before the change is refused (`ANSWER_TIMEOUT`). */
export const ANSWER_MS = 300_000;

/** `queue` with `request` at its end, unless it is there already. */
export function enqueue<R extends { id: string }>(queue: readonly Waiting<R>[], request: R, now: number): Waiting<R>[] {
  return queue.some((w) => w.request.id === request.id) ? [...queue] : [...queue, { request, since: now }];
}

/** `queue` without the request `id`. */
export function settle<R extends { id: string }>(queue: readonly Waiting<R>[], id: string): Waiting<R>[] {
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

/** A piece of a statement as the dialog shows it: its text, or a run of blank lines or of
 *  spaces collapsed into a visible marker. */
export type StatementPart = { text: string } | { gap: "lines" | "spaces"; count: number };

/** Blank lines in a row, and spaces in a row inside a line, from which they are collapsed. */
export const GAP_LINES = 2;
export const GAP_SPACES = 16;

/** Line breaks, whatever the platform or the Unicode separator. */
const BREAKS = new RegExp("\\r\\n|[\\n\\r\\u000b\\u000c\\u0085\\u2028\\u2029]");
/** Space that shows nothing: blanks, tabs and Unicode's invisible ones. */
const BLANK = "[ \\t\\u00a0\\u1680\\u180e\\u2000-\\u200d\\u202f\\u205f\\u2060\\u3000\\ufeff]";
const BLANK_LINE = new RegExp(`^${BLANK}*$`);
const SPACE_RUN = new RegExp(`${BLANK}{${GAP_SPACES},}`, "g");

/** `statement` as the dialog shows it: padding cannot push its end out of sight, so runs of
 *  blank lines and of spaces become markers that say how many there are. Everything else is
 *  kept as it is. */
export function statementParts(statement: string): StatementPart[] {
  const parts: StatementPart[] = [];
  const text = (s: string) => {
    if (!s) return;
    const last = parts[parts.length - 1];
    if (last && "text" in last) last.text += s;
    else parts.push({ text: s });
  };
  const lines = statement.split(BREAKS);
  for (let i = 0; i < lines.length; ) {
    let blank = 0;
    while (i + blank < lines.length && BLANK_LINE.test(lines[i + blank] ?? "")) blank++;
    if (blank >= GAP_LINES) {
      parts.push({ gap: "lines", count: blank });
      i += blank;
      if (i < lines.length) text("\n");
      continue;
    }
    const line = lines[i] ?? "";
    let at = 0;
    for (const run of line.matchAll(SPACE_RUN)) {
      const start = run.index ?? 0;
      text(line.slice(at, start));
      parts.push({ gap: "spaces", count: run[0].length });
      at = start + run[0].length;
    }
    text(line.slice(at));
    i++;
    if (i < lines.length) text("\n");
  }
  return parts;
}

/** The verdict of a refusal, with its reason when one was typed. */
export function denial(reason: string): Denial {
  const trimmed = reason.trim();
  return { answer: "deny", reason: trimmed ? trimmed : null };
}
