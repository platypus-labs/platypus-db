import { workbench } from "../workbench";
import { t } from "../i18n";

let open = 0;

/** A question is waiting: the shell's shortcuts wait too, as they did behind `window.confirm`. */
export const isAsking = () => open > 0;

interface Options {
  /** The confirming button's label, by default OK. */
  confirm?: string;
  /** The answer deletes or discards something: the button is red, and the focus starts on
   *  Cancel, so an Enter pressed by habit loses nothing. */
  danger?: boolean;
}

interface Choice<T> {
  label: string;
  value: T;
  /** Red: it deletes or discards something. */
  danger?: boolean;
  /** Drawn as the main answer. */
  primary?: boolean;
}

/** Asks `message` with `choices` beside Cancel; resolves to the chosen value, or `null` on
 *  Cancel, Escape or a click outside. The focus starts on the first choice that loses nothing,
 *  else on Cancel. */
export function choose<T>(message: string, choices: Choice<T>[]): Promise<T | null> {
  open++;
  let answered = false;
  const safe = choices.find((c) => !c.danger);
  return new Promise((resolve) =>
    workbench().openDialog((close) => {
      const answer = (value: T | null) => {
        if (answered) return;
        answered = true;
        open--;
        close();
        resolve(value);
      };
      return (
        <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && answer(null)}>
          <div
            className="modal confirm"
            role="alertdialog"
            aria-modal="true"
            aria-describedby="confirm-message"
            onKeyDown={(e) => e.key === "Escape" && answer(null)}
          >
            <p id="confirm-message">{message}</p>
            <div className="actions">
              <span className="spacer" />
              <button type="button" autoFocus={!safe} onClick={() => answer(null)}>
                {t("Cancel")}
              </button>
              {choices.map((c) => (
                <button
                  key={c.label}
                  type="button"
                  className={[c.primary && "primary", c.danger && "danger"].filter(Boolean).join(" ")}
                  autoFocus={c === safe}
                  onClick={() => answer(c.value)}
                >
                  {c.label}
                </button>
              ))}
            </div>
          </div>
        </div>
      );
    }),
  );
}

/** Asks a yes/no question in the app's own dialog, in place of `window.confirm`: resolves to
 *  `true` once confirmed, `false` on Cancel, Escape or a click outside. */
export async function ask(message: string, { confirm = t("OK"), danger = false }: Options = {}): Promise<boolean> {
  return (await choose(message, [{ label: confirm, value: true, danger, primary: true }])) === true;
}
