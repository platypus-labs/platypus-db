// Moving and maximizing the window from the app's own title bar on Linux.
//
// Tauri's drag regions start the window manager's move at the first press: on Linux the
// window manager then holds the pointer, so the second click of a double-click never
// reaches the page and the bar does not maximize. There the bar starts the move only once
// the pointer moves with the button held, and maximizes on a double-click.

/** The attribute that marks the bar's parts that move the window, as Tauri's own
 *  `data-tauri-drag-region`: only presses on the element itself count. */
export const DRAG_ATTR = "data-window-drag";

/** How far, in pixels, the pointer moves with the button held before the move starts. */
const SLOP = 3;

/** What the handlers need of the window. */
export interface DragWindow {
  startDragging(): Promise<void>;
  toggleMaximize(): Promise<void>;
}

/** The pointer events the handlers read. */
interface PointerLike {
  button: number;
  buttons: number;
  clientX: number;
  clientY: number;
  target: EventTarget | null;
}

const isDragPart = (target: EventTarget | null) =>
  !!target && typeof (target as Element).hasAttribute === "function" && (target as Element).hasAttribute(DRAG_ATTR);

/** Mouse handlers for the bar. */
export function dragHandlers(win: DragWindow, onError: (e: unknown) => void = () => {}) {
  let pressed: { x: number; y: number } | null = null;
  return {
    onMouseDown: (e: PointerLike) => {
      pressed = e.button === 0 && isDragPart(e.target) ? { x: e.clientX, y: e.clientY } : null;
    },
    onMouseMove: (e: PointerLike) => {
      if (!pressed) return;
      if ((e.buttons & 1) === 0) {
        pressed = null;
        return;
      }
      if (Math.abs(e.clientX - pressed.x) < SLOP && Math.abs(e.clientY - pressed.y) < SLOP) return;
      pressed = null;
      win.startDragging().catch(onError);
    },
    onMouseUp: () => {
      pressed = null;
    },
    onDoubleClick: (e: PointerLike) => {
      if (e.button === 0 && isDragPart(e.target)) win.toggleMaximize().catch(onError);
    },
  };
}
