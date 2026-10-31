/**
 * How many agent proposals open without asking: a few within a minute. More are asked about
 * once while the server keeps the next ones waiting (it keeps a bounded number), so an agent
 * cannot fill the tab bar.
 */

/** Proposals that open without asking within `RECENT_MS`. */
export const FEW = 3;
export const RECENT_MS = 60_000;

/** The times of `opened` within `RECENT_MS` before `now`. */
export function lately(opened: readonly number[], now: number): number[] {
  return opened.filter((at) => now - at < RECENT_MS);
}

/** Whether `count` more proposals, after those `opened` lately, are more than a few: the user
 *  is asked before they open. */
export function floods(opened: readonly number[], count: number, now: number): boolean {
  return lately(opened, now).length + count > FEW;
}