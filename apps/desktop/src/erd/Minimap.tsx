import { type PointerEvent, type RefObject, useEffect, useState } from "react";

interface MinimapRect {
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** What the canvas shows of the diagram, in canvas pixels. */
interface View {
  scrollWidth: number;
  scrollHeight: number;
  clientWidth: number;
  clientHeight: number;
  left: number;
  top: number;
  /** The space above the diagram, kept free for the floating toolbar. */
  padTop: number;
}

/** The size, in pixels, the overview fits into. */
const BOX = { width: 132, height: 92 };

/** An overview of the diagram: every table as a rectangle and the visible part as a frame.
 *  Clicking or dragging in it scrolls the canvas there. Hidden when the whole diagram fits. */
export function Minimap({ canvas, rects, zoom, version }: { canvas: RefObject<HTMLDivElement | null>; rects: MinimapRect[]; zoom: number; version: unknown }) {
  const [view, setView] = useState<View | null>(null);

  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const read = () =>
      setView({
        scrollWidth: el.scrollWidth,
        scrollHeight: el.scrollHeight,
        clientWidth: el.clientWidth,
        clientHeight: el.clientHeight,
        left: el.scrollLeft,
        top: el.scrollTop,
        padTop: parseFloat(getComputedStyle(el).paddingTop) || 0,
      });
    read();
    el.addEventListener("scroll", read, { passive: true });
    const observer = new ResizeObserver(read);
    observer.observe(el);
    return () => {
      el.removeEventListener("scroll", read);
      observer.disconnect();
    };
  }, [canvas, zoom, version]);

  if (!view || (view.scrollWidth <= view.clientWidth && view.scrollHeight <= view.clientHeight)) return null;
  const scale = Math.min(BOX.width / view.scrollWidth, BOX.height / view.scrollHeight);
  const width = view.scrollWidth * scale;
  const height = view.scrollHeight * scale;

  /** Scrolls the canvas so that the point under the pointer is the centre of what it shows. */
  const scrollTo = (e: PointerEvent<HTMLDivElement>) => {
    const el = canvas.current;
    if (!el) return;
    const box = e.currentTarget.getBoundingClientRect();
    el.scrollLeft = (e.clientX - box.left) / scale - view.clientWidth / 2;
    el.scrollTop = (e.clientY - box.top) / scale - view.clientHeight / 2;
  };
  const onDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    scrollTo(e);
  };
  const onMove = (e: PointerEvent<HTMLDivElement>) => {
    if (e.currentTarget.hasPointerCapture(e.pointerId)) scrollTo(e);
  };

  return (
    <div className="erd-minimap" aria-hidden="true" style={{ width, height }} onPointerDown={onDown} onPointerMove={onMove}>
      {rects.map((r) => (
        <i
          key={r.name}
          style={{
            left: r.x * zoom * scale,
            top: (view.padTop + r.y * zoom) * scale,
            width: Math.max(2, r.width * zoom * scale),
            height: Math.max(2, r.height * zoom * scale),
          }}
        />
      ))}
      <b className="erd-minimap-view" style={{ left: view.left * scale, top: view.top * scale, width: view.clientWidth * scale, height: view.clientHeight * scale }} />
    </div>
  );
}
