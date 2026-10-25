import { workbench } from "../workbench";

let open = 0;

/** A question is waiting: the shell's shortcuts wait too, as they did behind `window.confirm`. */
export const isAsking = () => open > 0;

interface Options {
  /** The confirming button's label, by default OK. */
  confirm?: string;
  /** The answer deletes or discards something: the button is red. */
  danger?: boolean;
}

/** Asks a yes/no question in the app's own dialog, in place of `window.confirm`: resolves to
 *  `true` once confirmed, `false` on Cancel, Escape or a click outside. */
export function ask(message: string, { confirm = "OK", danger = false }: Options = {}): Promise<boolean> {
  open++;
  let answered = false;
  return new Promise((resolve) =>
    workbench().openDialog((close) => {
      const answer = (yes: boolean) => {
        if (answered) return;
        answered = true;
        open--;
        close();
        resolve(yes);
      };
      return (
        <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && answer(false)}>
          <div
            className="modal small confirm"
            role="alertdialog"
            aria-modal="true"
            aria-describedby="confirm-message"
            onKeyDown={(e) => e.key === "Escape" && answer(false)}
          >
            <p id="confirm-message">{message}</p>
            <div className="actions">
              <span className="spacer" />
              <button type="button" onClick={() => answer(false)}>
                Cancel
              </button>
              <button type="button" className={`primary${danger ? " danger" : ""}`} autoFocus onClick={() => answer(true)}>
                {confirm}
              </button>
            </div>
          </div>
        </div>
      );
    }),
  );
}
