// What a statement whose `${name}` values went into its text asks for, judged again filled in.
import type { CheckedStatement } from "../guards/guards";

/** Whether the checks of `after` (a statement with its values filled in) ask for more than
 *  those of `before` (as written) did: more tables changed in every row, or more statements
 *  that change something. */
export function asksMore(before: CheckedStatement[], after: CheckedStatement[]): boolean {
  const unfiltered = (c: CheckedStatement[]) => c.reduce((n, s) => n + s.span.unfiltered.length, 0);
  const changes = (c: CheckedStatement[]) => c.filter((s) => s.span.kind !== "query" && s.span.kind !== "command").length;
  return unfiltered(after) > unfiltered(before) || changes(after) > changes(before);
}
