import { type PointerEvent as ReactPointerEvent, type RefObject, useEffect, useMemo, useRef, useState } from "react";
import { api, type ColumnInfo, type Geometries, type Shape, toCommandError } from "../api";
import type { GridApi } from "../grid/DataGrid";
import { t, plural } from "../i18n";

interface Props {
  jobId: number;
  columns: ColumnInfo[];
  grid: RefObject<GridApi | null>;
  /** Changes with the selection, to follow its columns. */
  selection: number;
  /** Loaded rows, and their version: more rows draw more shapes. */
  rows: number;
  version: number;
  /** Data row of the focused cell, drawn on top. */
  focusedRow: number | null;
  /** Focuses the cell of a clicked shape. */
  onPick: (row: number, col: number) => void;
  onClose: () => void;
}

interface Bounds {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** Centre in data coordinates and pixels per unit. */
interface View {
  cx: number;
  cy: number;
  scale: number;
}

const PADDING = 16;
const POINT_RADIUS = 3.5;
/** Waits for the rows to settle, e.g. while a result streams in. */
const DELAY_MS = 300;

/** The geometries of the selected columns in every loaded row, drawn on plane axes. */
export function MapPanel({ jobId, columns, grid, selection, rows, version, focusedRow, onPick, onClose }: Props) {
  const [cols, setCols] = useState<number[]>(() => grid.current?.selection()?.cols ?? []);
  const [data, setData] = useState<Geometries | null>(null);
  const [error, setError] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [view, setView] = useState<View | null>(null);
  const drag = useRef<{ x: number; y: number; view: View; moved: boolean } | null>(null);

  useEffect(() => {
    const next = grid.current?.selection()?.cols;
    if (next && next.join() !== cols.join()) setCols(next);
  }, [selection]);

  // Each fetch reads every loaded row again, so it waits for them to settle.
  useEffect(() => {
    let live = true;
    if (cols.length === 0) return;
    const timer = setTimeout(() => {
      api.geometries(jobId, 0, rows, cols).then(
        (g) => {
          if (!live) return;
          setData(g);
          setError(null);
        },
        (e) => live && setError(toCommandError(e).message),
      );
    }, DELAY_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [jobId, cols.join(), rows, version]);
  // Other columns or another result are shown whole again; more rows keep the view.
  useEffect(() => setView(null), [jobId, cols.join()]);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const bounds = useMemo(() => (data ? boundsOf(data.shapes.map((s) => s.shape)) : null), [data]);
  const fitted = bounds && size.w > 0 ? fit(bounds, size.w, size.h) : null;
  const v = view ?? fitted;

  const toScreen = (x: number, y: number): [number, number] =>
    v ? [size.w / 2 + (x - v.cx) * v.scale, size.h / 2 - (y - v.cy) * v.scale] : [0, 0];

  const onWheel = (e: React.WheelEvent) => {
    if (!v || !box.current) return;
    const rect = box.current.getBoundingClientRect();
    const factor = Math.exp(-e.deltaY / 500);
    // Zooms around the pointer: the point under it stays put.
    const px = e.clientX - rect.left - size.w / 2;
    const py = e.clientY - rect.top - size.h / 2;
    const x = v.cx + px / v.scale;
    const y = v.cy - py / v.scale;
    const scale = v.scale * factor;
    setView({ cx: x - px / scale, cy: y + py / scale, scale });
  };
  const onPointerDown = (e: ReactPointerEvent) => {
    if (!v || e.button !== 0) return;
    drag.current = { x: e.clientX, y: e.clientY, view: v, moved: false };
  };
  const onPointerMove = (e: ReactPointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.x;
    const dy = e.clientY - d.y;
    if (!d.moved && Math.hypot(dx, dy) < 3) return;
    if (!d.moved) (e.currentTarget as Element).setPointerCapture(e.pointerId);
    d.moved = true;
    setView({
      ...d.view,
      cx: d.view.cx - dx / d.view.scale,
      cy: d.view.cy + dy / d.view.scale,
    });
  };
  const onPointerUp = (e: ReactPointerEvent) => {
    const d = drag.current;
    drag.current = null;
    if (d?.moved) return;
    // A click, not a drag: the shape under the pointer, if any, picks its cell.
    const target = (e.target as Element).closest("[data-row]");
    if (target) onPick(Number(target.getAttribute("data-row")), Number(target.getAttribute("data-col")));
  };

  const shapes = data?.shapes ?? [];
  // The focused row last, so that it is drawn on top.
  const ordered =
    focusedRow === null
      ? shapes
      : [...shapes.filter((s) => s.row !== focusedRow), ...shapes.filter((s) => s.row === focusedRow)];
  const names = cols.map((c) => columns[c]?.name ?? "?").join(", ");

  return (
    <aside className="value-panel map-panel">
      <div className="value-bar">
        <strong className="small value-name" title={names}>
          {t("Map")}
          {names && ` · ${names}`}
        </strong>
        <span className="spacer" />
        <button className="icon" title={t("Show all")} aria-label={t("Show all")} disabled={!view} onClick={() => setView(null)}>
          ⤢
        </button>
        <button className="icon" title={t("Close")} aria-label={t("Close")} onClick={onClose}>
          ×
        </button>
      </div>
      {error && <div className="error-text small value-empty">{error}</div>}
      <div
        className="map-view"
        ref={box}
        onWheel={onWheel}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onDoubleClick={() => setView(null)}
      >
        {cols.length === 0 && <div className="muted small value-empty">{t("Select a column with geometries.")}</div>}
        {data && shapes.length === 0 && !error && (
          <div className="muted small value-empty">{t("No geometries in the selected columns.")}</div>
        )}
        {v && shapes.length > 0 && (
          <svg width={size.w} height={size.h}>
            {ordered.map((s) => (
              <g
                key={`${s.row}:${s.column}`}
                data-row={s.row}
                data-col={s.column}
                className={s.row === focusedRow ? "map-shape focused" : "map-shape"}
              >
                <title>{`${t("Row {n}", { n: s.row + 1 })} · ${columns[s.column]?.name ?? ""}`}</title>
                <ShapeView shape={s.shape} toScreen={toScreen} scale={v.scale} />
              </g>
            ))}
          </svg>
        )}
      </div>
      {data && (
        <div className="muted small value-size">
          {plural(shapes.length, "{n} geometry", "{n} geometries")}
          {data.truncated && ` ${t("(more not shown)")}`}
          {data.unreadable > 0 && ` · ${t("{n} not readable", { n: data.unreadable.toLocaleString() })}`}
        </div>
      )}
    </aside>
  );
}

function ShapeView({
  shape,
  toScreen,
  scale,
}: {
  shape: Shape;
  toScreen: (x: number, y: number) => [number, number];
  scale: number;
}) {
  const path = (points: [number, number][], close: boolean) =>
    points.map(([x, y], i) => `${i ? "L" : "M"}${toScreen(x, y).join(",")}`).join("") + (close ? "Z" : "");
  switch (shape.type) {
    case "point": {
      const [cx, cy] = toScreen(shape.x, shape.y);
      return <circle className="map-point" cx={cx} cy={cy} r={POINT_RADIUS} />;
    }
    case "line":
      return <path className="map-line" d={path(shape.points, false)} />;
    case "polygon":
      return <path className="map-area" fillRule="evenodd" d={shape.rings.map((r) => path(r, true)).join("")} />;
    case "circle": {
      const [cx, cy] = toScreen(shape.x, shape.y);
      return <circle className="map-area" cx={cx} cy={cy} r={Math.max(shape.r * scale, 1)} />;
    }
    case "collection":
      return (
        <>
          {shape.items.map((item, i) => (
            <ShapeView key={i} shape={item} toScreen={toScreen} scale={scale} />
          ))}
        </>
      );
  }
}

function boundsOf(shapes: Shape[]): Bounds | null {
  const b = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity };
  const add = (x: number, y: number, r = 0) => {
    b.x0 = Math.min(b.x0, x - r);
    b.y0 = Math.min(b.y0, y - r);
    b.x1 = Math.max(b.x1, x + r);
    b.y1 = Math.max(b.y1, y + r);
  };
  const visit = (s: Shape) => {
    switch (s.type) {
      case "point":
        return add(s.x, s.y);
      case "circle":
        return add(s.x, s.y, s.r);
      case "line":
        return s.points.forEach(([x, y]) => add(x, y));
      case "polygon":
        return s.rings.forEach((r) => r.forEach(([x, y]) => add(x, y)));
      case "collection":
        return s.items.forEach(visit);
    }
  };
  shapes.forEach(visit);
  return Number.isFinite(b.x0) ? b : null;
}

/** The view showing all of `b`; a single point gets a unit around it. */
function fit(b: Bounds, w: number, h: number): View {
  const bw = Math.max(b.x1 - b.x0, 1e-9);
  const bh = Math.max(b.y1 - b.y0, 1e-9);
  const single = b.x1 === b.x0 && b.y1 === b.y0;
  const scale = single ? 1 : Math.min((w - 2 * PADDING) / bw, (h - 2 * PADDING) / bh);
  return {
    cx: (b.x0 + b.x1) / 2,
    cy: (b.y0 + b.y1) / 2,
    scale: Math.max(scale, 1e-12),
  };
}
