// What the routine debugger computes without the server: the arguments of the call, the
// breakpoints, and what the tab shows of a state.
import type { DebugArgument, DebugParameter, DebugRoutine, DebugState } from "../api";

/** The parameters a call gives a value: `IN`, `INOUT` and `VARIADIC`. */
export function inputParameters(routine: DebugRoutine): DebugParameter[] {
  return routine.parameters.filter((p) => p.mode === "in" || p.mode === "inout" || p.mode === "variadic");
}

/** The arguments a call starts with: a parameter's default where it has one, else empty text. */
export function initialArguments(routine: DebugRoutine): DebugArgument[] {
  return inputParameters(routine).map((p) => (p.has_default ? { kind: "default" } : { kind: "value", value: "" }));
}

/** Why the arguments cannot be sent, as the index of the first wrong one: a default value
 *  can only be left out when every later argument is left out too. `null` when they can. */
export function misplacedDefault(args: DebugArgument[]): number | null {
  const given = args.map((a, i) => (a.kind === "default" ? -1 : i));
  const lastGiven = Math.max(-1, ...given);
  const i = args.findIndex((a, i) => a.kind === "default" && i < lastGiven);
  return i < 0 ? null : i;
}

/** A parameter as the dialog names it: its name, or its position. */
export function parameterName(p: DebugParameter, i: number): string {
  return p.name ?? `$${i + 1}`;
}

/** The key of a breakpoint: a line of a routine. */
const breakpointKey = (routine: string, line: number) => `${routine}:${line}`;

/** The lines of `routine` that have a breakpoint in `breakpoints`. */
export function breakpointLines(breakpoints: ReadonlySet<string>, routine: string): Set<number> {
  const prefix = `${routine}:`;
  return new Set([...breakpoints].filter((k) => k.startsWith(prefix)).map((k) => Number(k.slice(prefix.length))));
}

/** `breakpoints` with the one at `line` of `routine` set or cleared. */
export function withBreakpoint(breakpoints: ReadonlySet<string>, routine: string, line: number, on: boolean): Set<string> {
  const next = new Set(breakpoints);
  if (on) next.add(breakpointKey(routine, line));
  else next.delete(breakpointKey(routine, line));
  return next;
}

/** Whether commands (steps, breakpoints, variables) can run in `state`. */
export const isStopped = (state: DebugState | null): state is Extract<DebugState, { state: "stopped" }> =>
  state?.state === "stopped";

/** The state to show after `current` when `next` arrives (from an event or a command's
 *  answer, in any order): an ended call stays ended, and a stop's abort wins over the
 *  cancelled step that saw the call end. */
export function nextState(current: DebugState, next: DebugState): DebugState {
  if (current.state !== "finished") return next;
  return next.state === "finished" && next.outcome.aborted ? next : current;
}

/** Whether a function cannot be called directly: a trigger function. */
export const isTrigger = (routine: DebugRoutine) => routine.returns === "trigger" || routine.returns === "event_trigger";
