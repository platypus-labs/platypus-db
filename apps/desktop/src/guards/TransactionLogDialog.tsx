import { preview } from "./GuardDialog";
import { groups, type TxLog, type TxRecord } from "./txLog";
import { t, plural } from "../i18n";
import { ask } from "../ui/ask";

const time = (at: number) => new Date(at).toLocaleTimeString();

function ended(tx: TxRecord) {
  if (tx.ended === null) return { label: t("Open"), tone: "open" };
  if (tx.end === "ended") return { label: t("Ended at {time}", { time: time(tx.ended) }), tone: "ended" };
  return tx.end === "rollback"
    ? { label: t("Rolled back at {time}", { time: time(tx.ended) }), tone: "rollback" }
    : { label: t("Committed at {time}", { time: time(tx.ended) }), tone: "commit" };
}

/** The statements the tab ran in its session, newest first, gathered by transaction with how
 *  each ended; those that committed on their own stand alone. */
export function TransactionLogDialog({
  log,
  onRestore,
  onClear,
  onClose,
}: {
  log: TxLog;
  /** Opens the restore script of a statement's snapshot. */
  onRestore: (snapshot: string) => void;
  onClear: () => void;
  onClose: () => void;
}) {
  const all = groups(log);
  async function clear() {
    if (await ask(t("Clear the transaction log of this tab?"), { confirm: t("Clear"), danger: true })) onClear();
  }
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide tx-log" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Transaction log")}</h2>
        <p className="muted small">
          {all.length
            ? plural(log.entries.length, "{n} statement run in this tab's session, newest first.", "{n} statements run in this tab's session, newest first.")
            : t("Nothing has run in this tab's session yet.")}
        </p>
        {all.length > 0 && (
          <div className="tx-log-groups">
            {all.map(({ tx, entries }) => {
              const end = tx && ended(tx);
              return (
                <section key={tx ? `tx${tx.id}` : `at${entries[0]!.at}-${entries[0]!.sql}`} className={`tx-log-group ${end ? end.tone : "auto"}`}>
                  <header>
                    {tx ? (
                      <>
                        <strong>{t("Transaction from {time}", { time: time(tx.started) })}</strong>
                        <span className="tx-log-end">{end!.label}</span>
                      </>
                    ) : (
                      <span className="muted">{t("Auto-commit (outside a transaction)")}</span>
                    )}
                  </header>
                  <ol>
                    {[...entries].reverse().map((e, i) => (
                      <li key={i} className={e.failed ? "failed" : undefined}>
                        <span className="tx-log-time">{time(e.at)}</span>
                        <code>{preview(e.sql)}</code>
                        {e.failed && <span className="tx-log-failed">{t("Failed")}</span>}
                        {/* A change rolled back has nothing to restore. */}
                        {e.snapshot && tx?.end !== "rollback" && (
                          <button type="button" className="link tx-log-restore" onClick={() => onRestore(e.snapshot!)} title={t("Open the statements that put back the rows it changed")}>
                            {t("Restore…")}
                          </button>
                        )}
                      </li>
                    ))}
                  </ol>
                </section>
              );
            })}
          </div>
        )}
        <div className="actions">
          <button type="button" onClick={() => void clear()} disabled={all.length === 0}>
            {t("Clear")}
          </button>
          <span className="spacer" />
          <button type="button" className="primary" onClick={onClose} autoFocus>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}
