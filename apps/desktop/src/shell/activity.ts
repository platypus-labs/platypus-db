/** What the app did in the background and the errors it showed, newest last, for the
 *  Activity dialog. In memory only: the log of this run. */
import { useSyncExternalStore } from "react";
import { toCommandError } from "../api";

export interface ActivityEntry {
  id: number;
  /** `Date.now()` when it began. */
  at: number;
  kind: "error" | "task";
  text: string;
  /** A task's state; an error is `failed`. */
  status: "running" | "done" | "failed";
  /** How long a task took, once it ended. */
  ms?: number;
  /** Why a task failed. */
  detail?: string;
}

/** Older entries are dropped. */
const LIMIT = 500;

let entries: ActivityEntry[] = [];
/** Errors since the dialog was last opened. */
let unseen = 0;
let nextId = 1;
const listeners = new Set<() => void>();
let snapshot = { entries, unseen };

function changed() {
  snapshot = { entries, unseen };
  listeners.forEach((l) => l());
}

function add(entry: Omit<ActivityEntry, "id" | "at">): number {
  const id = nextId++;
  entries = [...entries.slice(-(LIMIT - 1)), { ...entry, id, at: Date.now() }];
  if (entry.kind === "error") unseen++;
  changed();
  return id;
}

export function logError(text: string) {
  add({ kind: "error", text, status: "failed" });
}

/** Logs `label` while `work` runs, and how it ended. */
export function track<T>(label: string, work: Promise<T>): Promise<T> {
  const id = add({ kind: "task", text: label, status: "running" });
  const started = Date.now();
  const end = (status: "done" | "failed", detail?: string) => {
    entries = entries.map((e) => (e.id === id ? { ...e, status, detail, ms: Date.now() - started } : e));
    changed();
  };
  work.then(
    () => end("done"),
    (e: unknown) => end("failed", toCommandError(e).message),
  );
  return work;
}

export function markSeen() {
  if (unseen === 0) return;
  unseen = 0;
  changed();
}

export function clearActivity() {
  entries = entries.filter((e) => e.status === "running");
  unseen = 0;
  changed();
}

export function useActivity(): { entries: ActivityEntry[]; unseen: number } {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => snapshot,
  );
}
