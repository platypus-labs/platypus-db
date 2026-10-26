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
 *  statement can finish before `runQuery` returns its job. `abandon` ends the run without
 *  waiting, for a backend that never answers `runQuery`: Cancel has no job to stop yet and
 *  closing the tab has nobody to tell. */
export function runGate(limited: boolean, resolve: (status: ResultStatus) => void) {
  let final: ResultStatus | null = null;
  let known = false;
  let over = false;
  let dropped = false;
  const end = (status: ResultStatus) => {
    if (over) return;
    over = true;
    resolve(status);
  };
  const finish = () => {
    if (final && known) end(final);
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
    /** The run is still waiting for `runQuery` to return its job. */
    awaitsJob: () => !known && !over,
    /** Resolves with `status` now, if the run is not over yet; what the run reports after
     *  that is no longer its own (see `abandoned`). */
    abandon(status: ResultStatus) {
      if (over) return;
      dropped = true;
      end(status);
    },
    /** `abandon` ended the run. */
    abandoned: () => dropped,
  };
}
