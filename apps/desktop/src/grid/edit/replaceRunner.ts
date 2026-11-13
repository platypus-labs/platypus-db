/**
 * Runs the mass edit's find and replace in a worker, away from the UI thread, so no regular
 * expression can freeze the app. The worker reports its progress while it works; one that
 * stays silent for `stallMs` is stuck on a text (catastrophic backtracking) and is stopped.
 */
import type { ReplaceJob, ReplaceReply, TextReplaced } from "./massEdit";

/** The part of a `Worker` a run uses. */
export interface WorkerLike {
  onmessage: ((e: { data: ReplaceReply }) => void) | null;
  onerror: ((e: unknown) => void) | null;
  postMessage(job: ReplaceJob): void;
  terminate(): void;
}

/** The replaced texts, one per text of the job (`null` where nothing matched); or the run
 *  was stopped as too slow; or the expression could not run. */
export type RunResult = { results: (TextReplaced | null)[] } | { slow: true } | { error: string };

export interface ReplaceRun {
  /** Settles once the run ends; never after `cancel`. */
  result: Promise<RunResult>;
  /** Stops the worker. */
  cancel(): void;
}

/** A worker silent for this long is stuck on one text. */
const STALL_MS = 2000;

/** Starts `job` in a worker from `createWorker`; `onProgress` gets the texts done so far. */
export function runReplace(
  job: ReplaceJob,
  createWorker: () => WorkerLike,
  onProgress: (done: number) => void,
  stallMs = STALL_MS,
): ReplaceRun {
  const worker = createWorker();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let finished = false;
  let settle: (result: RunResult) => void = () => {};
  const result = new Promise<RunResult>((resolve) => {
    settle = resolve;
  });
  const stop = () => {
    finished = true;
    clearTimeout(timer);
    worker.terminate();
  };
  const finish = (r: RunResult) => {
    if (finished) return;
    stop();
    settle(r);
  };
  const watch = () => {
    clearTimeout(timer);
    timer = setTimeout(() => finish({ slow: true }), stallMs);
  };
  worker.onmessage = ({ data }) => {
    if (finished) return;
    if ("done" in data) {
      watch();
      onProgress(data.done);
    } else {
      finish(data);
    }
  };
  worker.onerror = (e) => finish({ error: (e as { message?: string } | null)?.message ?? "the search failed" });
  watch();
  worker.postMessage(job);
  return { result, cancel: stop };
}
