import { useEffect } from "react";
import { workbench } from "../workbench";
import { type ActivityEntry, clearActivity, markSeen, useActivity } from "./activity";
import { formatCount, t } from "../i18n";

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
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2>{t("Activity")}</h2>
        <p className="muted small">{t("Errors shown and work done in the background since the app started.")}</p>
        {newest.length === 0 ? (
          <p className="muted">{t("Nothing yet.")}</p>
        ) : (
          <ol className="activity-list" aria-live="polite">
            {newest.map((e) => (
              <li key={e.id} className={`activity-entry ${e.kind} ${e.status}`}>
                <time>{time(e.at)}</time>
                <span className="activity-state">{t(e.kind === "error" ? "Error" : e.status === "running" ? "Running" : e.status === "done" ? "Done" : "Failed")}</span>
                <span className="activity-text">
                  {e.text}
                  {e.detail && <span className="activity-detail">{e.detail}</span>}
                </span>
                {e.ms !== undefined && <span className="muted activity-ms">{formatCount(e.ms)} ms</span>}
              </li>
            ))}
          </ol>
        )}
        <div className="actions">
          <button type="button" disabled={entries.length === 0} onClick={clearActivity}>
            {t("Clear")}
          </button>
          <button
            type="button"
            disabled={entries.length === 0}
            onClick={() => void navigator.clipboard.writeText(entries.map(line).join("\n")).catch(() => {})}
          >
            {t("Copy")}
          </button>
          <span className="spacer" />
          <button type="button" className="primary" autoFocus onClick={onClose}>
            {t("Done")}
          </button>
        </div>
      </div>
    </div>
  );
}
