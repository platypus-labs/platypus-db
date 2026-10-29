/** Handles that resize a panel by dragging, the arrow keys, or a double-click back to the
 *  default, with the size kept in the settings file (`layout.*`) for the next start. */
import { useState } from "react";
import { getStored, setStored } from "../settings";

/** A size kept under `key`: the one saved, else `fallback`. */
export function useStoredSize(key: string, fallback: number): [number, (size: number) => void] {
  const [size, setSize] = useState(() => {
    const saved = getStored(key);
    return typeof saved === "number" ? saved : fallback;
  });
  return [size, setSize];
}

const save = (key: string, size: number, fallback: number) => setStored(key, size === fallback ? null : size).catch(() => {});

interface Props {
  /** Where the size is kept. */
  storeKey: string;
  size: number;
  fallback: number;
  min: number;
  /** The largest the size may be now (the window's room). */
  max: () => number;
  /** `row`: a horizontal bar that sets a height; `column`: a vertical bar that sets a width. */
  direction: "row" | "column";
  /** The panel is after the handle (on its right, or below): moving towards it shrinks it. */
  after?: boolean;
  /** Pixels a unit of the size takes (a share of the room, for one), 1 by default. */
  scale?: () => number;
  /** While dragging, without rendering the app again: e.g. a CSS variable. */
  onDrag?: (size: number) => void;
  onResize: (size: number) => void;
  label: string;
  className?: string;
}

/** The handle; the size is set (and saved) when the drag ends, `onDrag` follows it before. */
export function PanelResizer({ storeKey, size, fallback, min, max, direction, after, scale, onDrag, onResize, label, className }: Props) {
  const [dragging, setDragging] = useState(false);
  const clamp = (n: number) => Math.round(Math.max(min, Math.min(Math.max(min, max()), n)));
  const set = (next: number) => {
    onResize(next);
    save(storeKey, next, fallback);
  };

  function start(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    const handle = e.currentTarget;
    const from = direction === "row" ? e.clientY : e.clientX;
    let last = size;
    handle.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent) => {
      const moved = ((direction === "row" ? ev.clientY : ev.clientX) - from) / (scale?.() || 1);
      last = clamp(size + (after ? -moved : moved));
      (onDrag ?? onResize)(last);
    };
    const onEnd = () => {
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onEnd);
      handle.removeEventListener("pointercancel", onEnd);
      handle.removeEventListener("lostpointercapture", onEnd);
      setDragging(false);
      if (last !== size) set(last);
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onEnd);
    handle.addEventListener("pointercancel", onEnd);
    handle.addEventListener("lostpointercapture", onEnd);
    setDragging(true);
  }

  function onKey(e: React.KeyboardEvent<HTMLDivElement>) {
    const back = direction === "row" ? "ArrowUp" : "ArrowLeft";
    const forth = direction === "row" ? "ArrowDown" : "ArrowRight";
    const step = e.key === back ? -16 : e.key === forth ? 16 : 0;
    if (!step && e.key !== "Home") return;
    e.preventDefault();
    set(e.key === "Home" ? fallback : clamp(size + (after ? -step : step)));
  }

  return (
    <div
      className={`panel-resizer ${direction}${dragging ? " dragging" : ""}${className ? ` ${className}` : ""}`}
      role="separator"
      aria-orientation={direction === "row" ? "horizontal" : "vertical"}
      aria-label={label}
      aria-valuenow={size}
      aria-valuemin={min}
      tabIndex={0}
      onPointerDown={start}
      onDoubleClick={() => set(fallback)}
      onKeyDown={onKey}
    />
  );
}
