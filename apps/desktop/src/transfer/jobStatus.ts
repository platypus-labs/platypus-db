/** The status of a transfer job (export, migration, generated rows), following its events. */
import type { ExportEvent } from "../api";

/** A job as its dialog shows it. A failed or stopped job keeps the rows written before. */
export type JobStatus =
  | { state: "idle" }
  | { state: "running"; id: number | null; rows: number }
  | { state: "done"; rows: number; warnings: string[] }
  | { state: "failed"; message: string; rows: number }
  | { state: "cancelled"; rows: number };

/** The status after `event`. */
export function afterEvent(status: JobStatus, event: ExportEvent): JobStatus {
  const rows = status.state === "running" ? status.rows : 0;
  switch (event.type) {
    case "progress":
      return status.state === "running" ? { ...status, rows: event.rows } : status;
    case "failed":
      return { state: "failed", message: event.error.message, rows };
    case "cancelled":
      return { state: "cancelled", rows };
    case "done":
      return { state: "done", rows: event.rows, warnings: event.warnings ?? [] };
  }
}
