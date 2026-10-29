import { type ReactNode, useState } from "react";
import { api, type ExportEvent, toCommandError } from "../api";
import { t } from "../i18n";
import { afterEvent, type JobStatus } from "./jobStatus";

export type { JobStatus };

/** The status of a job and how to start one, following its events. */
export function useJob() {
  const [status, setStatus] = useState<JobStatus>({ state: "idle" });
  const fail = (e: unknown) => setStatus({ state: "failed", message: toCommandError(e).message, rows: 0 });

  /** Starts the job `begin` spawns; `onDone` runs once it has finished, `onStopped` once it
   *  has failed or been stopped (for a job whose written rows stay). */
  async function start(begin: (onEvent: (e: ExportEvent) => void) => Promise<number>, onDone?: () => void, onStopped?: () => void) {
    setStatus({ state: "running", id: null, rows: 0 });
    try {
      const id = await begin((event) => {
        setStatus((s) => afterEvent(s, event));
        if (event.type === "done") onDone?.();
        else if (event.type !== "progress") onStopped?.();
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
  /** The notices while it runs and once done, with `{n}` for the rows and marked with `msg`,
   *  e.g. `["Copying… {n} rows", "Copied {n} rows."]`. */
  messages: [string, string];
  /** Put before a failure's message; a function gets the rows written before. */
  failed?: string | ((rows: number) => string);
  /** The notice once stopped, with `{n}` for the rows written and marked with `msg`; none
   *  for a job that leaves nothing behind. */
  stopped?: string;
  stop?: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const { status, messages, failed = "", stopped, stop = t("Stop"), onClose, children } = props;
  const notice = status.state === "cancelled" && stopped !== undefined;
  return (
    <>
      {status.state === "running" && (
        <div className="notice small">
          {t(messages[0], { n: status.rows.toLocaleString() })}
          {status.bytes && <progress className="job-progress" max={status.bytes[1] || 1} value={status.bytes[0]} />}
        </div>
      )}
      {status.state === "done" && (
        <div className={`notice small${status.warnings.length ? " warn" : ""}`}>
          {t(messages[1], { n: status.rows.toLocaleString() })}
          {status.warnings.length > 0 && (
            <>
              {" "}
              {t("Left out:")}
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
          {typeof failed === "function" ? failed(status.rows) : failed}
          {status.message}
        </div>
      )}
      {notice && <div className="notice warn small">{t(stopped, { n: status.rows.toLocaleString() })}</div>}
      <div className="actions">
        <span className="spacer" />
        {status.state === "running" ? (
          <button type="button" disabled={status.id === null} onClick={() => status.id !== null && void api.cancelExport(status.id)}>
            {stop}
          </button>
        ) : (
          <button type="button" onClick={onClose}>
            {t(status.state === "done" || notice ? "Close" : "Cancel")}
          </button>
        )}
        {children}
      </div>
    </>
  );
}
