/**
 * Dragging inside the page with the pointer, instead of HTML5 drag and drop.
 *
 * Tauri takes the drags of the webview to hand files dropped from outside to the app
 * (`onDragDropEvent`). On Windows, WebView2 then never gives the page the `dragover` and
 * `drop` of its own HTML5 drags, so nothing dragged inside the app could be dropped. These
 * drags follow the pointer events instead, which reach the page on every platform: a press
 * becomes a drag once the pointer moves a few pixels with the button held, a label follows
 * the pointer, the target under it is read with `elementFromPoint`, and Escape cancels.
 * A press that does not move stays a click.
 *
 * Mouse only: a touch or pen press is left to the browser (scrolling, tapping), as the
 * draggable parts do not take `touch-action: none`.
 */

/** How far, in pixels, the pointer moves with the button held before a press drags. */
export const DRAG_THRESHOLD = 4;

export interface Point {
  x: number;
  y: number;
}

/** Whether the pointer, now at `at`, is far enough from the press at `from` to drag. */
export function pastThreshold(from: Point, at: Point, threshold = DRAG_THRESHOLD): boolean {
  return Math.hypot(at.x - from.x, at.y - from.y) >= threshold;
}

/** Where in a target the pointer is: its first `edge` part is before it, its last after
 *  it, the rest inside. `offset` is from the target's top, `size` its height. */
export function edgeZone(offset: number, size: number, edge = 0.25): "before" | "inside" | "after" {
  if (offset < size * edge) return "before";
  return offset >= size * (1 - edge) ? "after" : "inside";
}

/** What `markedTarget` reads of an element. */
export interface ElementLike {
  getAttribute(name: string): string | null;
  readonly parentElement: ElementLike | null;
}

/** The element at or above `el` that carries `attr`, with the attribute's value; `null`
 *  when there is none. Drop targets mark themselves with a data attribute. */
export function markedTarget<E extends ElementLike>(el: E | null, attr: string): { element: E; value: string } | null {
  for (let at: ElementLike | null = el; at; at = at.parentElement) {
    const value = at.getAttribute(attr);
    if (value !== null) return { element: at as E, value };
  }
  return null;
}

/** What `startsDrag` reads of an element. */
export interface PressedLike extends ElementLike {
  readonly tagName: string;
  readonly parentElement: PressedLike | null;
}

/** Controls a press on which never drags: it stays theirs (typing, choosing, clicking). */
const CONTROLS = new Set(["INPUT", "TEXTAREA", "SELECT", "BUTTON"]);

/** Whether a press on `target` may drag `source`, the element it is inside: not when it
 *  lands on a control inside the source, an editable part, or an element marked
 *  `data-no-drag` (e.g. a row's twisty). The source itself may be a button. */
export function startsDrag(target: PressedLike | null, source: PressedLike | null): boolean {
  for (let at = target; at && at !== source; at = at.parentElement) {
    if (CONTROLS.has(at.tagName.toUpperCase()) || at.getAttribute("data-no-drag") !== null) return false;
    const editable = at.getAttribute("contenteditable");
    if (editable !== null && editable !== "false") return false;
  }
  return source === null || source.getAttribute("data-no-drag") === null;
}

export interface PointerDragOptions<T> {
  /** The text of the label that follows the pointer. */
  label: string;
  /** What a drop here would do, read from the element under the pointer; `null`: nothing. */
  target(el: Element | null, at: Point): T | null;
  /** Shows where the drop would land; `null` clears it, also at the end of the drag. */
  show?(target: T | null): void;
  /** Drops on `target`; `up` is the release, for its modifier keys. */
  drop(target: T, at: Point, up: PointerEvent): void;
}

/** The press that may start a drag: a React or DOM pointer event. */
export interface Press {
  button: number;
  isPrimary: boolean;
  pointerId: number;
  pointerType: string;
  clientX: number;
  clientY: number;
  target: EventTarget | null;
  currentTarget: EventTarget | null;
}

/** How long, in milliseconds, a swallowed click may take to come after the release. */
const CLICK_WAIT = 300;

/** Swallows the click the release of a drag makes on what was pressed: the first one, if
 *  it comes soon. */
function swallowClick() {
  const swallow = (e: Event) => {
    e.preventDefault();
    e.stopPropagation();
    done();
  };
  const done = () => {
    window.removeEventListener("click", swallow, true);
    clearTimeout(timer);
  };
  const timer = setTimeout(done, CLICK_WAIT);
  window.addEventListener("click", swallow, true);
}

/** Swallows the click of the release still to come of a drag cancelled while the button is
 *  held; a new press or leaving the window means that release will not come. */
function swallowReleaseClick() {
  const release = () => {
    forget();
    swallowClick();
  };
  const forget = () => {
    window.removeEventListener("pointerup", release, true);
    window.removeEventListener("pointerdown", forget, true);
    window.removeEventListener("blur", forget);
  };
  window.addEventListener("pointerup", release, true);
  window.addEventListener("pointerdown", forget, true);
  window.addEventListener("blur", forget);
}

const asElement = (t: EventTarget | null) => (t instanceof Element ? t : null);

/** Follows a mouse press of the main button (call it from `onPointerDown`): once the
 *  pointer moves with the button held, it is a drag of `options`; otherwise nothing happens
 *  and the press stays a click. Returns what cancels it, e.g. when its owner unmounts. */
export function pointerDrag<T>(press: Press, options: PointerDragOptions<T>): () => void {
  const source = asElement(press.currentTarget);
  if (press.button !== 0 || !press.isPrimary || press.pointerType !== "mouse" || !startsDrag(asElement(press.target), source)) {
    return () => {};
  }
  const from = { x: press.clientX, y: press.clientY };
  let ghost: HTMLElement | null = null;
  let stopped = false;

  const targetAt = (at: Point) => options.target(document.elementFromPoint(at.x, at.y), at);

  function begin() {
    ghost = document.createElement("div");
    ghost.className = "drag-ghost";
    ghost.textContent = options.label;
    document.body.append(ghost);
    document.documentElement.classList.add("pointer-dragging");
    window.getSelection()?.removeAllRanges();
    try {
      source?.setPointerCapture(press.pointerId);
    } catch {
      // The pointer is gone already; the window's listeners still follow it.
    }
  }

  /** Ends the drag, without a drop: removes what it shows and stops following the pointer. */
  function stop() {
    if (stopped) return;
    stopped = true;
    for (const [type, listener] of listeners) window.removeEventListener(type, listener, true);
    document.removeEventListener("visibilitychange", onHidden, true);
    if (!ghost) return;
    ghost.remove();
    ghost = null;
    document.documentElement.classList.remove("pointer-dragging");
    options.show?.(null);
  }

  function onMove(e: PointerEvent) {
    if (e.pointerId !== press.pointerId) return;
    // Released where the page could not see it, e.g. outside the window.
    if ((e.buttons & 1) === 0) return stop();
    const at = { x: e.clientX, y: e.clientY };
    if (!ghost) {
      if (!pastThreshold(from, at)) return;
      begin();
    }
    e.preventDefault();
    ghost!.style.transform = `translate(${at.x + 12}px, ${at.y + 8}px)`;
    options.show?.(targetAt(at));
  }

  function onUp(e: PointerEvent) {
    if (e.pointerId !== press.pointerId) return;
    const dragged = ghost !== null;
    stop();
    if (!dragged) return;
    swallowClick();
    const at = { x: e.clientX, y: e.clientY };
    const target = targetAt(at);
    if (target !== null) options.drop(target, at, e);
  }

  /** The pointer is no longer the drag's: cancelled by the browser, or capture taken away. */
  function onLost(e: PointerEvent) {
    if (e.pointerId === press.pointerId) stop();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || !ghost) return;
    e.preventDefault();
    e.stopPropagation();
    stop();
    swallowReleaseClick();
  }

  /** No menu while dragging: the right button does nothing then. */
  function onMenu(e: Event) {
    if (!ghost) return;
    e.preventDefault();
    e.stopPropagation();
  }

  /** Alt+Tab and the like: the release may never reach the page. Only the window's own
   *  blur: those of the page's elements pass the window's capture too. */
  function onBlur(e: Event) {
    if (e.target === window) stop();
  }

  function onHidden() {
    if (document.visibilityState === "hidden") stop();
  }

  const listeners: [string, EventListener][] = [
    ["pointermove", onMove as EventListener],
    ["pointerup", onUp as EventListener],
    ["pointercancel", onLost as EventListener],
    ["lostpointercapture", onLost as EventListener],
    ["keydown", onKey as EventListener],
    ["contextmenu", onMenu],
    ["blur", onBlur],
  ];
  for (const [type, listener] of listeners) window.addEventListener(type, listener, true);
  document.addEventListener("visibilitychange", onHidden, true);
  return stop;
}
