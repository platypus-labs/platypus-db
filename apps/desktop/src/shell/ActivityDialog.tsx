import { useEffect } from "react";
import { workbench } from "../workbench";
import { type ActivityEntry, clearActivity, markSeen, useActivity } from "./activity";

/** Opens the log of the errors shown and the work done in the background. */
export function openActivity() {
  workbench().openDialog((close) => <ActivityDialog onClose={close} />);
}

const time = (at: number) => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

const line = (e: ActivityEntry) =>
  `${time(e.at)}  ${e.kind === "error" ? "error" : e.status}  ${e.text}${e.ms !== undefined ? ` (${e.ms} ms)` : ""}${e.detail ? `: ${e.detail}` : ""}`;

function ActivityDialog({ onClose }: { onClose: () => void }) {
  const { entries } = useActivity();
  // Errors that arrive while it is open are seen too.
  useEffect(markSeen, [entries]);
  const newest = [...entries].reverse();
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="modal activity"
        role="dialog"
        aria-modal="true"
        aria-labelledby="activity-title"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2 id="activity-title">Activity</h2>
        <p className="muted small">Errors shown and work done in the background since the app started.</p>
        {newest.length === 0 ? (
          <p className="muted activity-empty">Nothing yet.</p>
        ) : (
          <ol className="activity-list" aria-live="polite">
            {newest.map((e) => (
              <li key={e.id} className={`activity-entry ${e.kind} ${e.status}`}>
                <time>{time(e.at)}</time>
                <span className="activity-state">{e.kind === "error" ? "Error" : e.status === "running" ? "Running" : e.status === "done" ? "Done" : "Failed"}</span>
                <span className="activity-text">
                  {e.text}
                  {e.detail && <span className="activity-detail">{e.detail}</span>}
                </span>
                {e.ms !== undefined && <span className="muted activity-ms">{e.ms} ms</span>}
              </li>
            ))}
          </ol>
        )}
        <div className="actions">
          <button type="button" disabled={entries.length === 0} onClick={clearActivity}>
            Clear
          </button>
          <button
            type="button"
            disabled={entries.length === 0}
            onClick={() => void navigator.clipboard.writeText(entries.map(line).join("\n")).catch(() => {})}
          >
            Copy
          </button>
          <span className="spacer" />
          <button type="button" className="primary" autoFocus onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
