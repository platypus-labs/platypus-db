/**
 * What assistive technology needs from every dialog (`.modal`), in one place instead of in
 * each of them: the dialog role, a name from its heading, the focus moved inside while it is
 * open and back where it was once it closes, and Tab kept within the dialog on top.
 */

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [contenteditable="true"], [tabindex]:not([tabindex="-1"])';

let titles = 0;

/** What Tab reaches, in order: shown, and not taken out of the sequence with a negative
 *  tabindex (as the unselected tabs of a tab list are). */
const focusables = (root: HTMLElement) =>
  [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (el) => el.getClientRects().length > 0 && !(Number(el.getAttribute("tabindex")) < 0),
  );

/** The element that had the focus before each open dialog. */
const openers = new WeakMap<HTMLElement, Element | null>();

/** The element focused last, and the one before it: a field with autoFocus takes the focus
 *  before the dialog is seen here, so the opener is then the one before. */
let focused: Element | null = null;
let before: Element | null = null;

/** Dialogs, and the command palette, which gives the focus back the same way. */
const LAYERS = ".modal, .palette";

function prepare(modal: HTMLElement) {
  if (modal.matches(".palette")) {
    const active = document.activeElement;
    openers.set(modal, modal.contains(active) ? before : active);
    return;
  }
  if (!modal.hasAttribute("role")) modal.setAttribute("role", "dialog");
  modal.setAttribute("aria-modal", "true");
  if (!modal.hasAttribute("aria-labelledby") && !modal.hasAttribute("aria-label")) {
    const heading = modal.querySelector("h2, h3");
    if (heading) {
      heading.id ||= `dialog-title-${++titles}`;
      modal.setAttribute("aria-labelledby", heading.id);
    }
  }
  const active = document.activeElement;
  openers.set(modal, modal.contains(active) ? before : active);
  // A field with autoFocus keeps it; otherwise the first control, or the dialog itself.
  requestAnimationFrame(() => {
    if (!modal.isConnected || modal.contains(document.activeElement)) return;
    const first = focusables(modal)[0];
    if (first) first.focus();
    else {
      modal.tabIndex = -1;
      modal.focus();
    }
  });
}

function closed(modal: HTMLElement) {
  const opener = openers.get(modal);
  // Only when the focus went nowhere, so a dialog that opened something else leaves it there.
  if (opener instanceof HTMLElement && opener.isConnected && (document.activeElement === document.body || !document.activeElement)) {
    opener.focus();
  }
}

const modalsIn = (node: Node): HTMLElement[] =>
  node instanceof HTMLElement ? [...(node.matches(LAYERS) ? [node] : []), ...node.querySelectorAll<HTMLElement>(LAYERS)] : [];

/** Escape with the focus nowhere (after a click on a dialog's blank area) still reaches the
 *  dialog on top, whose own handler closes it. */
function escapeToTop(e: KeyboardEvent) {
  if (e.key !== "Escape" || e.defaultPrevented) return;
  const active = document.activeElement;
  if (e.target !== document.body || (active && active !== document.body)) return;
  const top = [...document.querySelectorAll<HTMLElement>(LAYERS)].filter((m) => m.getClientRects().length > 0).at(-1);
  top?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, cancelable: true }));
}

function trapTab(e: KeyboardEvent) {
  if (e.key !== "Tab") return;
  // A dialog left open in a hidden tab is not the one on top.
  const all = [...document.querySelectorAll<HTMLElement>(".modal")].filter((m) => m.getClientRects().length > 0);
  const top = all.at(-1);
  if (!top) return;
  const list = focusables(top);
  if (list.length === 0) return;
  const first = list[0]!;
  const last = list.at(-1)!;
  const active = document.activeElement;
  if (!top.contains(active)) first.focus();
  else if (e.shiftKey && active === first) last.focus();
  else if (!e.shiftKey && active === last) first.focus();
  else return;
  e.preventDefault();
}

export function startModalA11y() {
  document.querySelectorAll<HTMLElement>(".modal").forEach(prepare);
  new MutationObserver((records) => {
    for (const r of records) {
      r.addedNodes.forEach((n) => modalsIn(n).forEach(prepare));
      r.removedNodes.forEach((n) => modalsIn(n).forEach(closed));
    }
  }).observe(document.body, { childList: true, subtree: true });
  document.addEventListener(
    "focusin",
    (e) => {
      if (e.target === focused) return;
      before = focused;
      focused = e.target as Element;
    },
    true,
  );
  document.addEventListener("keydown", trapTab, true);
  document.addEventListener("keydown", escapeToTop);
}
