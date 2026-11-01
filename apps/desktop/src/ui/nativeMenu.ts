/** The part of an element `keepsNativeMenu` looks at. */
interface Target {
  tagName?: string;
  type?: string;
  isContentEditable?: boolean;
  closest?: (selector: string) => unknown;
}

/** Inputs that hold no text to cut, copy or paste. */
const NOT_TEXT = new Set(["checkbox", "radio", "button", "submit", "reset", "range", "color", "file", "image"]);

/** Whether a right click on `el` may show the browser's own menu: only in a plain text field
 *  (cut, copy, paste), never in the code editor, which has its menu. */
export function keepsNativeMenu(el: Target | null): boolean {
  if (!el || el.closest?.(".cm-editor")) return false;
  if (el.tagName === "INPUT") return !NOT_TEXT.has(el.type ?? "text");
  return el.tagName === "TEXTAREA" || el.isContentEditable === true;
}

/** No browser menu anywhere else: a right click that no app menu took is swallowed. */
export function startNativeMenuGuard() {
  document.addEventListener("contextmenu", (e) => {
    if (!e.defaultPrevented && !keepsNativeMenu(e.target as Element | null)) e.preventDefault();
  });
}
