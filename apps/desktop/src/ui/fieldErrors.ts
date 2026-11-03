/** A field the browser finds invalid on submit (empty and required, a number out of range…)
 *  shows the app's own message under it, in the interface's language, instead of the
 *  browser's bubble, which is drawn by the system and speaks Windows' language. */
import { msg, t } from "../i18n";
import { type FieldErrorKind, fieldError } from "./fieldError";

/** The message of each kind, with `{n}` for its value. */
const MESSAGES: Record<Exclude<FieldErrorKind, "own">, string> = {
  tick: msg("Tick this box."),
  fill: msg("Fill in this field."),
  min: msg("At least {n}."),
  max: msg("At most {n}."),
  short: msg("At least {n} characters."),
  number: msg("Enter a number."),
  invalid: msg("This value is not valid."),
};

let shown: { field: HTMLElement; pop: HTMLElement; done: () => void } | null = null;

function hide() {
  shown?.done();
  shown = null;
}

function show(field: HTMLInputElement) {
  hide();
  const { kind, value } = fieldError(field);
  const pop = document.createElement("div");
  pop.className = "field-error-pop";
  pop.setAttribute("role", "alert");
  pop.textContent = kind === "own" ? String(value) : t(MESSAGES[kind], { n: value ?? "" });
  document.body.appendChild(pop);
  const r = field.getBoundingClientRect();
  pop.style.left = `${Math.max(4, Math.min(r.left, window.innerWidth - pop.offsetWidth - 4))}px`;
  pop.style.top = `${r.bottom + 4 + pop.offsetHeight > window.innerHeight ? r.top - pop.offsetHeight - 4 : r.bottom + 4}px`;
  field.classList.add("field-invalid");
  // Typing in the field, leaving it, or anything moving it ends the message.
  const end = () => hide();
  field.addEventListener("input", end);
  field.addEventListener("blur", end);
  window.addEventListener("resize", end);
  document.addEventListener("scroll", end, true);
  shown = {
    field,
    pop,
    done: () => {
      pop.remove();
      field.classList.remove("field-invalid");
      field.removeEventListener("input", end);
      field.removeEventListener("blur", end);
      window.removeEventListener("resize", end);
      document.removeEventListener("scroll", end, true);
    },
  };
  field.focus({ preventScroll: true });
}

export function startFieldErrors() {
  // A submit checks every field: the first invalid one gets the message.
  let first = true;
  document.addEventListener(
    "invalid",
    (e) => {
      e.preventDefault();
      if (!first) return;
      first = false;
      queueMicrotask(() => (first = true));
      const field = e.target as HTMLInputElement;
      // On the next frame: a dialog may first show the page that holds the field.
      requestAnimationFrame(() => {
        field.scrollIntoView({ block: "nearest" });
        show(field);
      });
    },
    true,
  );
}
