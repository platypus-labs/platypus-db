import { toCommandError } from "../api";
import { t } from "../i18n";
import { workbench } from "../workbench";

export type InitChoice = "run" | "skip";

/** How long an answer is shared with sessions opened right after it (a tab and its navigator
 *  connect together). */
const SHARE_MS = 2000;

/** The answers being waited for, by connection: sessions opened together share one question. */
const asking = new Map<string, Promise<InitChoice>>();

/** Shows the init statements a shared project brought, read-only, and asks whether to run
 *  them. Closing the dialog connects without them. */
function askInit(name: string, sql: string): Promise<InitChoice> {
  return new Promise((resolve) =>
    workbench().openDialog((close) => {
      const answer = (choice: InitChoice) => {
        close();
        resolve(choice);
      };
      return (
        <div className="modal-backdrop">
          <div
            className="modal wide"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="init-approval-title"
            data-testid="init-approval"
            onKeyDown={(e) => e.key === "Escape" && answer("skip")}
          >
            <h2 id="init-approval-title">{t("Run the init statements of {name}?", { name })}</h2>
            <p className="muted">
              {t("This shared project sets SQL to run every time {name} connects. Read it before you allow it; if it changes, you are asked again.", { name })}
            </p>
            <pre className="admin-statement init-approval-sql" data-testid="init-approval-sql">
              {sql}
            </pre>
            <div className="actions">
              <span className="spacer" />
              <button type="button" autoFocus onClick={() => answer("skip")}>
                {t("Connect without them")}
              </button>
              <button type="button" className="primary" onClick={() => answer("run")}>
                {t("Run them and remember")}
              </button>
            </div>
          </div>
        </div>
      );
    }),
  );
}

/** Runs `open`, which connects; if the connection's init statements come from a shared
 *  project and are not approved on this machine, asks first and opens again with the answer. */
export async function withInitApproval<T>(
  connection: { id: string; name: string },
  open: (init?: InitChoice) => Promise<T>,
): Promise<T> {
  try {
    return await open();
  } catch (e) {
    const error = toCommandError(e);
    if (error.kind !== "init_approval") throw e;
    let answer = asking.get(connection.id);
    if (!answer) {
      answer = askInit(connection.name, error.message).finally(() => setTimeout(() => asking.delete(connection.id), SHARE_MS));
      asking.set(connection.id, answer);
    }
    return open(await answer);
  }
}
