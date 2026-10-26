import { preview } from "./GuardDialog";
import { t, plural } from "../i18n";

/** The statements of the open transaction that are not committed yet, with Commit and
 *  Rollback, which wait while statements run, as the toolbar's do. `onEnd` gets `true` to
 *  commit, `false` to roll back, `null` to close. */
export function PendingDialog({
  statements,
  running,
  onEnd,
}: {
  statements: string[];
  running: boolean;
  onEnd: (commit: boolean | null) => void;
}) {
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onEnd(null)}>
      <div className="modal wide guard" onKeyDown={(e) => e.key === "Escape" && onEnd(null)}>
        <h2>{t("Pending transaction")}</h2>
        {statements.length ? (
          <>
            <p className="muted small">
              {plural(statements.length, "{n} statement waits for Commit:", "{n} statements wait for Commit:")}
            </p>
            <ol className="guard-statements">
              {statements.map((sql, i) => (
                <li key={i}>
                  <code>{preview(sql)}</code>
                </li>
              ))}
            </ol>
          </>
        ) : (
          <p className="muted small">{t("No change run from this tab yet: the transaction may only hold reads and locks.")}</p>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={() => onEnd(null)} autoFocus>
            {t("Close")}
          </button>
          <button type="button" className="danger" onClick={() => onEnd(false)} disabled={running}>
            {t("Rollback")}
          </button>
          <button type="button" className="primary" onClick={() => onEnd(true)} disabled={running}>
            {t("Commit")}
          </button>
        </div>
      </div>
    </div>
  );
}
