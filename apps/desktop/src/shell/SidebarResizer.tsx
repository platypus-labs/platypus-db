import { useEffect, useState } from "react";
import { getStored, setStored } from "../settings";
import { t } from "../i18n";

/** Where the width of the connections column is kept, in pixels. */
const KEY = "layout.sidebar_width";
/** Where earlier builds kept it: still read, removed on the next change. */
const OLD_KEY = "ui.sidebar_width";
/** Also the fallback of `--sidebar-width` in shell.css. */
const DEFAULT = 260;
const MIN = 180;
const STEP = 16;

/** The widest the column may be: room is left for the work. */
const maxWidth = () => Math.max(MIN, Math.min(640, window.innerWidth - 420));

function clamp(width: number): number {
  return Math.round(Math.max(MIN, Math.min(maxWidth(), width)));
}

/** The width of the connections column: the one saved, else the default; narrowed when
 *  the window gets too small for it. */
export function useSidebarWidth(): [number, (width: number) => void] {
  const [width, setWidth] = useState(() => {
    const saved = getStored(KEY) ?? getStored(OLD_KEY);
    return typeof saved === "number" ? clamp(saved) : DEFAULT;
  });
  useEffect(() => {
    const onResize = () => setWidth((w) => clamp(w));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return [width, (w) => setWidth(clamp(w))];
}

/** One write after the other: each saves the whole file. */
const save = (width: number) =>
  void (async () => {
    if (getStored(OLD_KEY) !== undefined) await setStored(OLD_KEY, null);
    await setStored(KEY, width === DEFAULT ? null : width);
  })().catch(() => {});

/** The handle between the connections and the work: dragged, or the arrows once focused,
 *  set the width; a double-click goes back to the default. Saved when the drag ends. */
export function SidebarResizer({ width, onResize }: { width: number; onResize: (width: number) => void }) {
  const [dragging, setDragging] = useState(false);
  const resizeAndSave = (next: number) => {
    onResize(next);
    save(next);
  };

  /** While dragging only the shell's variable changes, so the app is not rendered again at
   *  every move; the width is set and saved once, at the end. */
  function start(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    const handle = e.currentTarget;
    const shell = handle.closest<HTMLElement>(".shell");
    const startX = e.clientX;
    let last = width;
    handle.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent) => {
      last = clamp(width + ev.clientX - startX);
      shell?.style.setProperty("--sidebar-width", `${last}px`);
    };
    // Released, cancelled by the system, or the capture lost (e.g. the window left).
    const onEnd = () => {
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onEnd);
      handle.removeEventListener("pointercancel", onEnd);
      handle.removeEventListener("lostpointercapture", onEnd);
      setDragging(false);
      if (last !== width) resizeAndSave(last);
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onEnd);
    handle.addEventListener("pointercancel", onEnd);
    handle.addEventListener("lostpointercapture", onEnd);
    setDragging(true);
  }

  function onKey(e: React.KeyboardEvent<HTMLDivElement>) {
    const delta = e.key === "ArrowLeft" ? -STEP : e.key === "ArrowRight" ? STEP : 0;
    if (!delta && e.key !== "Home") return;
    e.preventDefault();
    resizeAndSave(e.key === "Home" ? DEFAULT : clamp(width + delta));
  }

  return (
    <div
      className={`sidebar-resizer${dragging ? " dragging" : ""}`}
      role="separator"
      aria-orientation="vertical"
      aria-label={t("Resize the connections")}
      aria-valuenow={width}
      aria-valuemin={MIN}
      aria-valuemax={maxWidth()}
      tabIndex={0}
      title={t("Drag to resize, double-click to reset")}
      onPointerDown={start}
      onDoubleClick={() => resizeAndSave(DEFAULT)}
      onKeyDown={onKey}
    />
  );
}
