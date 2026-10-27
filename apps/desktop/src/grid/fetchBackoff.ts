/**
 * Blocks of rows whose fetch failed, so the grid does not ask for them again at every redraw:
 * each failure waits longer before the next try (1 s, 2 s, 4 s… up to 30 s).
 */

const FIRST_MS = 1000;
const MAX_MS = 30_000;

export class FailedBlocks {
  #failed = new Map<number, { attempts: number; until: number }>();

  /** Records a failed fetch of `block` at `now`; returns the wait before the next try. */
  fail(block: number, now: number): number {
    const attempts = (this.#failed.get(block)?.attempts ?? 0) + 1;
    const wait = Math.min(FIRST_MS * 2 ** (attempts - 1), MAX_MS);
    this.#failed.set(block, { attempts, until: now + wait });
    return wait;
  }

  /** `block` was fetched: it starts afresh. */
  ok(block: number) {
    this.#failed.delete(block);
  }

  /** Whether `block` may be fetched at `now`. */
  ready(block: number, now: number): boolean {
    const f = this.#failed.get(block);
    return !f || now >= f.until;
  }

  /** Some block is failing. */
  get any(): boolean {
    return this.#failed.size > 0;
  }
}
