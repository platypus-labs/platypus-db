import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import type { ColumnInfo } from "../api";
import type { RowCache } from "./RowCache";

const ROW_H = 24;
const HEADER_H = 40;
const PAD = 8;
const MIN_COL_W = 48;
const MAX_INITIAL_COL_W = 360;
/** Browsers cap element height (~33M px in Chromium); larger results scroll proportionally. */
const MAX_SCROLL_PX = 10_000_000;
const RESIZE_HIT_PX = 4;
/** Rows measured to size columns when a result first appears. */
const SIZE_SAMPLE_ROWS = 200;
const FONT = "12px system-ui, -apple-system, 'Segoe UI', sans-serif";
const FONT_BOLD = "600 12px system-ui, -apple-system, 'Segoe UI', sans-serif";
const FONT_SMALL = "11px system-ui, -apple-system, 'Segoe UI', sans-serif";
const FONT_NULL = "italic 12px system-ui, -apple-system, 'Segoe UI', sans-serif";

interface Cell {
  row: number;
  col: number;
}

interface Props {
  columns: ColumnInfo[];
  rowCount: number;
  cache: RowCache;
  /** Bumped whenever the cache receives rows. */
  version: number;
}

interface Theme {
  bg: string;
  bgAlt: string;
  header: string;
  fg: string;
  muted: string;
  border: string;
  selection: string;
  focus: string;
}

function readTheme(el: Element): Theme {
  const s = getComputedStyle(el);
  const v = (name: string) => s.getPropertyValue(name).trim();
  return {
    bg: v("--grid-bg"),
    bgAlt: v("--grid-bg-alt"),
    header: v("--grid-header"),
    fg: v("--fg"),
    muted: v("--fg-muted"),
    border: v("--border"),
    selection: v("--grid-selection"),
    focus: v("--accent"),
  };
}

export function DataGrid({ columns, rowCount, cache, version }: Props) {
  const outerRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const spacerRef = useRef<HTMLDivElement>(null);
  const widths = useRef<number[]>([]);
  const sizedToData = useRef(false);
  const anchor = useRef<Cell | null>(null);
  const focus = useRef<Cell | null>(null);
  const resizing = useRef<{ col: number; startX: number; startW: number } | null>(null);
  const selecting = useRef(false);
  const theme = useRef<Theme | null>(null);
  const frame = useRef(0);
  // Latest props for the imperative draw loop.
  const props = useRef({ columns, rowCount, cache });
  props.current = { columns, rowCount, cache };

  const measure = useCallback((text: string, font: string) => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!ctx) return text.length * 7;
    ctx.font = font;
    return ctx.measureText(text).width;
  }, []);

  // Header-based widths until the first rows arrive, then sized to fit them (see `draw`).
  useLayoutEffect(() => {
    widths.current = columns.map((c) => headerWidth(c));
    sizedToData.current = false;
    anchor.current = focus.current = null;
    // headerWidth only depends on measure.
  }, [columns, measure]);

  function headerWidth(c: ColumnInfo) {
    return Math.max(MIN_COL_W, measure(c.name, FONT_BOLD), measure(c.db_type, FONT_SMALL)) + PAD * 2;
  }

  /** Widens columns to fit the first rows, once per result. */
  function sizeToData() {
    const { columns, rowCount, cache } = props.current;
    if (sizedToData.current || rowCount === 0 || cache.get(0, 0) === undefined) return;
    sizedToData.current = true;
    const sample = Math.min(rowCount, SIZE_SAMPLE_ROWS);
    widths.current = columns.map((c, col) => {
      let max = headerWidth(c);
      for (let r = 0; r < sample; r++) {
        const v = cache.get(r, col);
        if (v) max = Math.max(max, measure(v, FONT) + PAD * 2);
      }
      return Math.min(MAX_INITIAL_COL_W, Math.ceil(max));
    });
  }

  const gutterWidth = () => Math.ceil(measure(String(props.current.rowCount), FONT)) + PAD * 2;

  const contentHeight = () => HEADER_H + props.current.rowCount * ROW_H;

  /** Maps the DOM scroll position to a logical one, for results taller than the browser allows. */
  const logicalScrollTop = () => {
    const el = scrollRef.current;
    if (!el) return 0;
    const real = contentHeight();
    const dom = Math.min(real, MAX_SCROLL_PX);
    if (real === dom || dom <= el.clientHeight) return el.scrollTop;
    return (el.scrollTop * (real - el.clientHeight)) / (dom - el.clientHeight);
  };

  const columnX = (col: number) => {
    let x = 0;
    for (let i = 0; i < col; i++) x += widths.current[i] ?? 0;
    return x;
  };

  const draw = useCallback(() => {
    frame.current = 0;
    const canvas = canvasRef.current;
    const scroller = scrollRef.current;
    const outer = outerRef.current;
    if (!canvas || !scroller || !outer) return;
    const { columns, rowCount, cache } = props.current;
    const t = (theme.current ??= readTheme(outer));
    sizeToData();
    // Column resizes change the scrollable width without a React render.
    const contentWidth = gutterWidth() + columnX(columns.length) + 200;
    if (spacerRef.current) spacerRef.current.style.width = `${contentWidth}px`;
    const w = scroller.clientWidth;
    const h = scroller.clientHeight;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = t.bg;
    ctx.fillRect(0, 0, w, h);
    ctx.textBaseline = "middle";

    const gutter = gutterWidth();
    const sx = scroller.scrollLeft;
    const sy = logicalScrollTop();
    const firstRow = Math.max(0, Math.floor(sy / ROW_H));
    const lastRow = Math.min(rowCount - 1, Math.floor((sy + h - HEADER_H) / ROW_H));
    const rowY = (r: number) => HEADER_H + r * ROW_H - sy;

    const sel = selectionRect();

    // Row backgrounds.
    for (let r = firstRow; r <= lastRow; r++) {
      if (r % 2 === 1) {
        ctx.fillStyle = t.bgAlt;
        ctx.fillRect(gutter, rowY(r), w - gutter, ROW_H);
      }
    }

    // Cells, column by column so each column is clipped once.
    let x = gutter - sx;
    for (let c = 0; c < columns.length; c++) {
      const cw = widths.current[c] ?? MIN_COL_W;
      if (x + cw > gutter && x < w) {
        const column = columns[c];
        ctx.save();
        ctx.beginPath();
        ctx.rect(Math.max(x, gutter), HEADER_H, cw - (x < gutter ? gutter - x : 0), h - HEADER_H);
        ctx.clip();
        if (sel && c >= sel.c0 && c <= sel.c1) {
          const y0 = Math.max(rowY(sel.r0), HEADER_H);
          const y1 = Math.min(rowY(sel.r1 + 1), h);
          if (y1 > y0) {
            ctx.fillStyle = t.selection;
            ctx.fillRect(x, y0, cw, y1 - y0);
          }
        }
        for (let r = firstRow; r <= lastRow; r++) {
          const y = rowY(r) + ROW_H / 2;
          const value = cache.get(r, c);
          if (value === undefined) {
            ctx.fillStyle = t.border;
            ctx.fillRect(x + PAD, y - 2, Math.min(cw - PAD * 2, 40), 4);
            continue;
          }
          if (value === null) {
            ctx.font = FONT_NULL;
            ctx.fillStyle = t.muted;
            ctx.textAlign = "left";
            ctx.fillText("NULL", x + PAD, y);
            continue;
          }
          ctx.font = FONT;
          ctx.fillStyle = t.fg;
          if (column?.numeric) {
            ctx.textAlign = "right";
            ctx.fillText(value, x + cw - PAD, y);
          } else {
            ctx.textAlign = "left";
            ctx.fillText(value, x + PAD, y);
          }
        }
        // Column separator.
        ctx.fillStyle = t.border;
        ctx.fillRect(x + cw - 1, HEADER_H, 1, h - HEADER_H);
        ctx.restore();
      }
      x += cw;
    }

    // Focused cell outline.
    const f = focus.current;
    if (f && f.row >= firstRow && f.row <= lastRow) {
      const fx = gutter - sx + columnX(f.col);
      const fw = widths.current[f.col] ?? MIN_COL_W;
      if (fx + fw > gutter) {
        ctx.strokeStyle = t.focus;
        ctx.lineWidth = 2;
        ctx.strokeRect(Math.max(fx, gutter) + 1, rowY(f.row) + 1, fw - 2, ROW_H - 2);
      }
    }

    // Row-number gutter, before the header so a partly scrolled-out row slides under it.
    ctx.fillStyle = t.header;
    ctx.fillRect(0, HEADER_H, gutter, h - HEADER_H);
    ctx.font = FONT;
    ctx.textAlign = "right";
    ctx.fillStyle = t.muted;
    for (let r = firstRow; r <= lastRow; r++) {
      ctx.fillText(String(r + 1), gutter - PAD, rowY(r) + ROW_H / 2);
    }

    // Header.
    ctx.fillStyle = t.header;
    ctx.fillRect(0, 0, w, HEADER_H);
    x = gutter - sx;
    ctx.save();
    ctx.beginPath();
    ctx.rect(gutter, 0, w - gutter, HEADER_H);
    ctx.clip();
    for (let c = 0; c < columns.length; c++) {
      const cw = widths.current[c] ?? MIN_COL_W;
      const column = columns[c];
      if (column && x + cw > gutter && x < w) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(x, 0, cw - 1, HEADER_H);
        ctx.clip();
        ctx.textAlign = "left";
        ctx.font = FONT_BOLD;
        ctx.fillStyle = t.fg;
        ctx.fillText(column.name, x + PAD, 14);
        ctx.font = FONT_SMALL;
        ctx.fillStyle = t.muted;
        ctx.fillText(column.db_type, x + PAD, 29);
        ctx.restore();
        ctx.fillStyle = t.border;
        ctx.fillRect(x + cw - 1, 6, 1, HEADER_H - 12);
      }
      x += cw;
    }
    ctx.restore();
    ctx.fillStyle = t.border;
    ctx.fillRect(0, HEADER_H - 1, w, 1);
    ctx.fillRect(gutter - 1, 0, 1, h);

    cache.ensure(firstRow, lastRow, rowCount);
    // Reads only refs, so it never needs to be recreated.
  }, []);

  const schedule = useCallback(() => {
    if (!frame.current) frame.current = requestAnimationFrame(draw);
  }, [draw]);

  function selectionRect() {
    const a = anchor.current;
    const f = focus.current;
    if (!a || !f) return null;
    return {
      r0: Math.min(a.row, f.row),
      r1: Math.max(a.row, f.row),
      c0: Math.min(a.col, f.col),
      c1: Math.max(a.col, f.col),
    };
  }

  // Redraw on data, size and theme changes.
  useEffect(schedule, [schedule, version, rowCount, columns]);
  useEffect(() => {
    const outer = outerRef.current;
    if (!outer) return;
    const observer = new ResizeObserver(schedule);
    observer.observe(outer);
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onTheme = () => {
      theme.current = null;
      schedule();
    };
    media.addEventListener("change", onTheme);
    return () => {
      observer.disconnect();
      media.removeEventListener("change", onTheme);
      cancelAnimationFrame(frame.current);
      frame.current = 0;
    };
  }, [schedule]);

  /** Grid coordinates under a mouse event. */
  function hit(e: React.MouseEvent) {
    const scroller = scrollRef.current!;
    const rect = scroller.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    const gutter = gutterWidth();
    const cx = px - gutter + scroller.scrollLeft;
    let col = -1;
    let border = -1;
    let x = 0;
    for (let c = 0; c < props.current.columns.length; c++) {
      const cw = widths.current[c] ?? MIN_COL_W;
      if (Math.abs(cx - (x + cw)) <= RESIZE_HIT_PX) border = c;
      if (cx >= x && cx < x + cw) col = c;
      x += cw;
    }
    const row = Math.floor((py - HEADER_H + logicalScrollTop()) / ROW_H);
    return { px, py, col, border, row, inHeader: py < HEADER_H, inGutter: px < gutter };
  }

  function scrollIntoView(cell: Cell) {
    const scroller = scrollRef.current!;
    const gutter = gutterWidth();
    const x = columnX(cell.col);
    const cw = widths.current[cell.col] ?? MIN_COL_W;
    const viewW = scroller.clientWidth - gutter;
    if (x < scroller.scrollLeft) scroller.scrollLeft = x;
    else if (x + cw > scroller.scrollLeft + viewW) scroller.scrollLeft = x + cw - viewW;

    const sy = logicalScrollTop();
    const viewH = scroller.clientHeight - HEADER_H;
    const y = cell.row * ROW_H;
    let target = sy;
    if (y < sy) target = y;
    else if (y + ROW_H > sy + viewH) target = y + ROW_H - viewH;
    if (target !== sy) {
      const real = contentHeight();
      const dom = Math.min(real, MAX_SCROLL_PX);
      scroller.scrollTop =
        real === dom ? target : (target * (dom - scroller.clientHeight)) / (real - scroller.clientHeight);
    }
  }

  function setFocus(cell: Cell, extend: boolean) {
    const { rowCount, columns } = props.current;
    const clamped = {
      row: Math.max(0, Math.min(rowCount - 1, cell.row)),
      col: Math.max(0, Math.min(columns.length - 1, cell.col)),
    };
    if (rowCount === 0 || columns.length === 0) return;
    focus.current = clamped;
    if (!extend || !anchor.current) anchor.current = clamped;
    scrollIntoView(clamped);
    schedule();
  }

  function autosize(col: number) {
    const scroller = scrollRef.current!;
    const { cache, columns } = props.current;
    const first = Math.floor(logicalScrollTop() / ROW_H);
    const visible = Math.ceil(scroller.clientHeight / ROW_H);
    const column = columns[col];
    if (!column) return;
    let max = Math.max(measure(column.name, FONT_BOLD), measure(column.db_type, FONT_SMALL));
    for (let r = first; r < first + visible * 3; r++) {
      const v = cache.get(r, col);
      if (v) max = Math.max(max, measure(v, FONT));
    }
    widths.current[col] = Math.max(MIN_COL_W, Math.ceil(max) + PAD * 2);
    schedule();
  }

  function onMouseDown(e: React.MouseEvent) {
    if (e.button !== 0) return;
    const h = hit(e);
    if (h.inHeader && h.border >= 0) {
      e.preventDefault();
      resizing.current = {
        col: h.border,
        startX: e.clientX,
        startW: widths.current[h.border] ?? MIN_COL_W,
      };
      return;
    }
    if (h.inHeader || h.row < 0 || h.row >= props.current.rowCount) return;
    if (h.inGutter) {
      anchor.current = { row: h.row, col: 0 };
      setFocus({ row: h.row, col: props.current.columns.length - 1 }, true);
      return;
    }
    if (h.col < 0) return;
    selecting.current = true;
    setFocus({ row: h.row, col: h.col }, e.shiftKey);
  }

  function onMouseMove(e: React.MouseEvent) {
    const scroller = scrollRef.current!;
    const r = resizing.current;
    if (r) {
      widths.current[r.col] = Math.max(MIN_COL_W, r.startW + e.clientX - r.startX);
      schedule();
      return;
    }
    const h = hit(e);
    scroller.style.cursor = h.inHeader && h.border >= 0 ? "col-resize" : "default";
    if (selecting.current && e.buttons === 1 && h.col >= 0 && !h.inHeader) {
      setFocus({ row: h.row, col: h.col }, true);
    }
  }

  function onMouseUp() {
    resizing.current = null;
    selecting.current = false;
  }

  function onDoubleClick(e: React.MouseEvent) {
    const h = hit(e);
    if (h.inHeader && h.border >= 0) autosize(h.border);
  }

  async function copySelection() {
    const sel = selectionRect();
    if (!sel) return;
    const { cache } = props.current;
    const lines: string[] = [];
    for (let r = sel.r0; r <= sel.r1; r++) {
      const cells: string[] = [];
      for (let c = sel.c0; c <= sel.c1; c++) cells.push(cache.get(r, c) ?? "");
      lines.push(cells.join("\t"));
    }
    await navigator.clipboard.writeText(lines.join("\n"));
  }

  function onKeyDown(e: React.KeyboardEvent) {
    const f = focus.current;
    const page = Math.max(1, Math.floor((scrollRef.current!.clientHeight - HEADER_H) / ROW_H) - 1);
    const ctrl = e.ctrlKey || e.metaKey;
    const moves: Record<string, () => Cell | null> = {
      ArrowUp: () => f && { ...f, row: f.row - 1 },
      ArrowDown: () => f && { ...f, row: f.row + 1 },
      ArrowLeft: () => f && { ...f, col: f.col - 1 },
      ArrowRight: () => f && { ...f, col: f.col + 1 },
      PageUp: () => f && { ...f, row: f.row - page },
      PageDown: () => f && { ...f, row: f.row + page },
      Home: () => f && (ctrl ? { row: 0, col: 0 } : { ...f, col: 0 }),
      End: () =>
        f &&
        (ctrl
          ? { row: props.current.rowCount - 1, col: props.current.columns.length - 1 }
          : { ...f, col: props.current.columns.length - 1 }),
    };
    const move = moves[e.key];
    if (move) {
      e.preventDefault();
      const next = move() ?? { row: 0, col: 0 };
      setFocus(next, e.shiftKey);
    } else if (ctrl && e.key.toLowerCase() === "a") {
      e.preventDefault();
      anchor.current = { row: 0, col: 0 };
      focus.current = {
        row: props.current.rowCount - 1,
        col: props.current.columns.length - 1,
      };
      schedule();
    } else if (ctrl && e.key.toLowerCase() === "c") {
      e.preventDefault();
      void copySelection();
    }
  }

  const spacerHeight = Math.min(HEADER_H + rowCount * ROW_H, MAX_SCROLL_PX);

  return (
    <div className="grid" ref={outerRef}>
      <canvas ref={canvasRef} className="grid-canvas" />
      <div
        ref={scrollRef}
        className="grid-scroll"
        tabIndex={0}
        onScroll={schedule}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onMouseLeave={onMouseUp}
        onDoubleClick={onDoubleClick}
        onKeyDown={onKeyDown}
      >
        <div ref={spacerRef} style={{ height: spacerHeight }} />
      </div>
    </div>
  );
}
