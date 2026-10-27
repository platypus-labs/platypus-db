/** Undo and redo of the data editor's pending changes: the states before each change. */
export class History<T> {
  private undone: T[] = [];
  private done: T[] = [];
  private readonly limit: number;

  /** Keeps the last `limit` states to undo. */
  constructor(limit: number) {
    this.limit = limit;
  }

  get canUndo(): boolean {
    return this.done.length > 0;
  }

  get canRedo(): boolean {
    return this.undone.length > 0;
  }

  /** A change left `previous` behind: it can be undone, and what was undone cannot be redone. */
  record(previous: T): void {
    this.done.push(previous);
    if (this.done.length > this.limit) this.done.shift();
    this.undone = [];
  }

  /** The state before `current`'s change; `undefined` when there is none. */
  undo(current: T): T | undefined {
    const previous = this.done.pop();
    if (previous !== undefined) this.undone.push(current);
    return previous;
  }

  /** The state `current` was undone from; `undefined` when there is none. */
  redo(current: T): T | undefined {
    const next = this.undone.pop();
    if (next !== undefined) this.done.push(current);
    return next;
  }

  clear(): void {
    this.done = [];
    this.undone = [];
  }
}
