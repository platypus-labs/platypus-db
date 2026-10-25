import { useState } from "react";
import { getStored, setStored } from "../settings";
import { t } from "../i18n";

/** Where the width of the connections column is kept, in pixels. */
const KEY = "layout.sidebar_width";
const DEFAULT = 260;
const MIN = 180;
const STEP = 16;

/** Wide enough to read, and leaving room for the work. */
function clamp(width: number): number {
  const max = Math.max(MIN, Math.min(640, window.innerWidth - 420));
  return Math.round(Math.max(MIN, Math.min(max, width)));
}

/** The width of the connections column: the one saved, else the default. */
export function useSidebarWidth(): [number, (width: number) => void] {
  const [width, setWidth] = useState(() => {
    const saved = getStored(KEY);
    return typeof saved === "number" ? clamp(saved) : DEFAULT;
  });
  return [width, (w) => setWidth(clamp(w))];
}

const save = (width: number) => void setStored(KEY, width === DEFAULT ? null : width).catch(() => {});

/** The handle between the connections and the work: dragged, or the arrows once focused,
 *  set the width; a double-click goes back to the default. Saved when the drag ends. */
export function SidebarResizer({ width, onResize }: { width: number; onResize: (width: number) => void }) {
  const [dragging, setDragging] = useState(false);
  const resizeAndSave = (next: number) => {
    onResize(next);
    save(next);
  };

  function start(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    const startX = e.clientX;
    let last = width;
    const onMove = (ev: PointerEvent) => {
      last = clamp(width + ev.clientX - startX);
      onResize(last);
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      setDragging(false);
      save(last);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
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
      tabIndex={0}
      title={t("Drag to resize, double-click to reset")}
      onPointerDown={start}
      onDoubleClick={() => resizeAndSave(DEFAULT)}
      onKeyDown={onKey}
    />
  );
}
