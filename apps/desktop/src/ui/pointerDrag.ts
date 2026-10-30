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
interface Press {
  button: number;
  isPrimary: boolean;
  pointerId: number;
  clientX: number;
  clientY: number;
  currentTarget: EventTarget | null;
}

/** Swallows the click the release of a drag would make on what was pressed. */
function swallowClick() {
  const stop = (e: Event) => {
    e.preventDefault();
    e.stopPropagation();
  };
  window.addEventListener("click", stop, true);
  setTimeout(() => window.removeEventListener("click", stop, true), 0);
}

/** Follows a press of the main button (call it from `onPointerDown`): once the pointer
 *  moves with the button held, it is a drag of `options`; otherwise nothing happens and the
 *  press stays a click. */
export function pointerDrag<T>(press: Press, options: PointerDragOptions<T>) {
  if (press.button !== 0 || !press.isPrimary) return;
  const from = { x: press.clientX, y: press.clientY };
  const source = press.currentTarget instanceof Element ? press.currentTarget : null;
  let ghost: HTMLElement | null = null;
  // Cancelled with Escape: the release still to come must not click.
  let cancelled = false;

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

  /** Ends what the drag shows. */
  function hide() {
    if (!ghost) return;
    ghost.remove();
    ghost = null;
    document.documentElement.classList.remove("pointer-dragging");
    options.show?.(null);
  }

  function stop() {
    window.removeEventListener("pointermove", onMove, true);
    window.removeEventListener("pointerup", onUp, true);
    window.removeEventListener("pointercancel", onCancel, true);
    window.removeEventListener("keydown", onKey, true);
    hide();
  }

  function onMove(e: PointerEvent) {
    if (e.pointerId !== press.pointerId || cancelled) return;
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
    if (dragged || cancelled) swallowClick();
    stop();
    if (!dragged) return;
    const at = { x: e.clientX, y: e.clientY };
    const target = targetAt(at);
    if (target !== null) options.drop(target, at, e);
  }

  function onCancel(e: PointerEvent) {
    if (e.pointerId === press.pointerId) stop();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || !ghost) return;
    e.preventDefault();
    e.stopPropagation();
    cancelled = true;
    // Only the release is left to follow, to swallow its click.
    hide();
  }

  window.addEventListener("pointermove", onMove, true);
  window.addEventListener("pointerup", onUp, true);
  window.addEventListener("pointercancel", onCancel, true);
  window.addEventListener("keydown", onKey, true);
}
