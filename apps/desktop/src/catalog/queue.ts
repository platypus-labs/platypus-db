// Tasks that must not overlap per key, e.g. the writes to the catalog of one connection. Pure, so
// it can be tested without the backend.

interface KeyedQueue {
  /** Runs `task` once the earlier ones of `key` are done, whether they succeeded or not. */
  run<T>(key: string, task: () => Promise<T>): Promise<T>;
  /** Forgets `key`; tasks already queued still run. */
  forget(key: string): void;
}

export function keyedQueue(): KeyedQueue {
  const tails = new Map<string, Promise<unknown>>();
  return {
    run(key, task) {
      const run = (tails.get(key) ?? Promise.resolve()).then(task);
      tails.set(key, run.catch(() => undefined));
      return run;
    },
    forget: (key) => void tails.delete(key),
  };
}
