// When the run of one statement is over: plain logic of QueryTab's `runOne`, tested on its
// own (tests/runGate.test.mjs).
import type { ResultStatus } from "./ResultPanel";

/** Whether `status` ends the statement's run. */
export function isTerminal(status: ResultStatus, limited: boolean): boolean {
  switch (status.kind) {
    case "affected":
    case "error":
      return true;
    case "rows":
      // Without a limit the job pauses on demand, so the first rows are the end of "running".
      return limited ? status.finished : true;
    default:
      return false;
  }
}

/** Calls `resolve` once with the first terminal status, but only after `jobKnown` too: a fast
 *  statement can finish before `runQuery` returns its job. */
export function runGate(limited: boolean, resolve: (status: ResultStatus) => void) {
  let final: ResultStatus | null = null;
  let known = false;
  const finish = () => {
    if (final && known) resolve(final);
  };
  return {
    settle(status: ResultStatus) {
      if (!final && isTerminal(status, limited)) {
        final = status;
        finish();
      }
    },
    jobKnown() {
      if (known) return;
      known = true;
      finish();
    },
  };
}
