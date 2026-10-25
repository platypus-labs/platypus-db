import { type ReactNode, useState } from "react";
import { api, type ExportEvent, toCommandError } from "../api";

/** A transfer job (export, migration, generated rows) as its dialog shows it. */
export type JobStatus =
  | { state: "idle" }
  | { state: "running"; id: number | null; rows: number }
  | { state: "done"; rows: number; warnings: string[] }
  | { state: "failed"; message: string };

/** The status of a job and how to start one, following its events. */
export function useJob() {
  const [status, setStatus] = useState<JobStatus>({ state: "idle" });
  const fail = (e: unknown) => setStatus({ state: "failed", message: toCommandError(e).message });

  /** Starts the job `begin` spawns; `onDone` runs once it has finished. */
  async function start(begin: (onEvent: (e: ExportEvent) => void) => Promise<number>, onDone?: () => void) {
    setStatus({ state: "running", id: null, rows: 0 });
    try {
      const id = await begin((event) => {
        if (event.type === "progress") setStatus((s) => (s.state === "running" ? { ...s, rows: event.rows } : s));
        else if (event.type === "failed") setStatus({ state: "failed", message: event.error.message });
        else if (event.type === "cancelled") setStatus({ state: "idle" });
        else {
          setStatus({ state: "done", rows: event.rows, warnings: event.warnings ?? [] });
          onDone?.();
        }
      });
      setStatus((s) => (s.state === "running" ? { ...s, id } : s));
    } catch (e) {
      fail(e);
    }
  }

  return { status, start, fail };
}

/** The progress or outcome of a job, then its Stop or Close button and `children`, the button
 *  that starts it. */
export function JobFooter(props: {
  status: JobStatus;
  /** The verbs of the notices, e.g. `["Copying", "Copied"]`. */
  verbs: [string, string];
  /** Put before a failure's message. */
  failed?: string;
  stop?: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const { status, verbs, failed = "", stop = "Stop", onClose, children } = props;
  return (
    <>
      {status.state === "running" && (
        <div className="notice small">
          {verbs[0]}… {status.rows.toLocaleString()} rows
        </div>
      )}
      {status.state === "done" && (
        <div className={`notice small${status.warnings.length ? " warn" : ""}`}>
          {verbs[1]} {status.rows.toLocaleString()} rows.
          {status.warnings.length > 0 && (
            <>
              {" "}
              Left out:
              <ul className="migrate-warnings">
                {status.warnings.map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
      {status.state === "failed" && (
        <div className="notice error small">
          {failed}
          {status.message}
        </div>
      )}
      <div className="actions">
        <span className="spacer" />
        {status.state === "running" ? (
          <button type="button" disabled={status.id === null} onClick={() => status.id !== null && void api.cancelExport(status.id)}>
            {stop}
          </button>
        ) : (
          <button type="button" onClick={onClose}>
            {status.state === "done" ? "Close" : "Cancel"}
          </button>
        )}
        {children}
      </div>
    </>
  );
}
