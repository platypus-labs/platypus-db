/**
 * What a SQL editor tab runs again without asking: after saving edited rows, on auto-refresh,
 * after reloading the tables of "Query with SQL". Only statements that passed the checks
 * before a run (read-only connection, UPDATE without WHERE, production) and started are
 * remembered, so a run the user declined never runs later unasked; and only statements that
 * only read run again: a change was confirmed for the run it was asked for, not for the next.
 */

export interface Rerunnable {
  readOnly: boolean;
}

/** Whether `planned` may run again without the checks before a run. */
export function rerunsUnasked(planned: readonly Rerunnable[]): boolean {
  return planned.length > 0 && planned.every((p) => p.readOnly);
}

export class RunMemory<P extends Rerunnable> {
  /** The statements of the last run that passed its checks. */
  #last: P[] = [];
  /** The statement each result ran, by result id. */
  #byResult = new Map<number, P>();

  /** A run passed its checks and starts; `fresh` when its results replace all others. */
  started(planned: P[], fresh: boolean) {
    this.#last = planned;
    if (fresh) this.#byResult.clear();
  }

  /** Result `id` shows what `p` ran: its statement was sent. */
  ran(id: number, p: P) {
    this.#byResult.set(id, p);
  }

  /** Result `id` is gone. */
  forget(id: number) {
    this.#byResult.delete(id);
  }

  /** The statements of the last run. */
  last(): readonly P[] {
    return this.#last;
  }

  /** The statements of the last run, when they may run again unasked; else `null`. */
  lastReads(): P[] | null {
    return rerunsUnasked(this.#last) ? this.#last : null;
  }

  /** The statement result `id` ran, when it may run again unasked; else `null`. */
  readOf(id: number): P | null {
    const p = this.#byResult.get(id);
    return p && rerunsUnasked([p]) ? p : null;
  }
}
