import {
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { dispatchKey } from "../actions";
import { type ColumnInfo, type SavedConnection, toCommandError } from "../api";
import { ContextMenu, type MenuState } from "../components/ContextMenu";
import { THEME_EVENT } from "../theme";
import { workbench } from "../workbench";
import { type ColumnLayout, EMPTY_LAYOUT, moveColumn, resolveLayout } from "./layout";
import type { RowCache } from "./RowCache";
import "./actions";

/** Row height, from the `--grid-row-h` variable (the density setting); see `readTheme`. */
let ROW_H = 24;
const HEADER_H = 40;
const PAD = 8;
const MIN_COL_W = 48;
const MAX_INITIAL_COL_W = 360;
/** Browsers cap element height (~33M px in Chromium); larger results scroll proportionally. */
const MAX_SCROLL_PX = 10_000_000;
const RESIZE_HIT_PX = 4;
/** Mouse travel that turns a header click into a column drag. */
const DRAG_PX = 5;
/** Rows measured to size columns when a result first appears. */
const SIZE_SAMPLE_ROWS = 200;
/** Canvas fonts, from `--font-mono` (set by `readTheme`, like `ROW_H`). */
const MONO_FALLBACK = '"Cascadia Code", Consolas, ui-monospace, monospace';
let FONT = `12px ${MONO_FALLBACK}`;
let FONT_BOLD = `600 12px ${MONO_FALLBACK}`;
let FONT_SMALL = `11px ${MONO_FALLBACK}`;
let FONT_NULL = `italic 12px ${MONO_FALLBACK}`;

/** A grid cell: `col` is the result column, wherever the layout shows it. */
export interface Cell {
  row: number;
  col: number;
}

/** Inclusive rows and the result columns of a rectangle of the grid. */
export interface GridSelection {
  r0: number;
  r1: number;
  /** Selected result columns, left to right as shown; hidden ones are left out. */
  cols: number[];
}

/** How a feature changes a cell's look, e.g. an edited value or a search match. */
export interface CellDecoration {
  background?: string;
  color?: string;
  /** Shown instead of the fetched value; `null` shows NULL. */
  text?: string | null;
  /** Struck through, e.g. a row marked for deletion. */
  strike?: boolean;
  /** Muted text after the value, e.g. the name of the row a foreign key points to. */
  hint?: string;
}

/**
 * What features can do with a grid: read values and the selection, move the selection,
 * place overlays (inline editors) over cells. Given to "grid" actions and through `ref`.
 * Columns are always result columns, whatever order the layout shows them in.
 */
export interface GridApi {
  columns(): ColumnInfo[];
  /** Every row, the `topRows` included. */
  rowCount(): number;
  /** The text of a cell as shown, decorations included: `null` for SQL NULL, `undefined`
   *  while not loaded. */
  value(row: number, col: number): string | null | undefined;
  selection(): GridSelection | null;
  focused(): Cell | null;
  /** The result row shown at a grid row; `null` for the `topRows`. */
  dataRow(row: number): number | null;
  select(anchor: Cell, focus?: Cell): void;
  selectAll(): void;
  /** The result columns shown, left to right. */
  visibleColumns(): number[];
  /** The column layout, or `null` when this grid's columns cannot be arranged. */
  layout(): ColumnLayout | null;
  setLayout(layout: ColumnLayout): void;
  /** A cell's box relative to the grid element, or `null` while scrolled out of view. */
  cellRect(row: number, col: number): { left: number; top: number; width: number; height: number } | null;
  redraw(): void;
  /** Gives the keyboard back to the grid, e.g. after an overlay closes. */
  focus(): void;
}

/**
 * What features add to `GridSource` for their actions, by augmenting this interface from
 * their own module, e.g. the data editor's `edit`.
 */
export interface GridSourceExtensions {}

/** Where the rows come from, for actions that need more than the values. */
export interface GridSource extends GridSourceExtensions {
  connection: SavedConnection;
  sql: string;
  jobId: number | null;
  /** Data viewer: the quoted, qualified table the rows come from. */
  table: string | null;
}

/** The argument of actions in the "grid" scope. */
export interface GridActionContext {
  grid: GridApi;
  source: GridSource | null;
}

interface Props {
  columns: ColumnInfo[];
  /** Rows of the result, in `cache`. */
  rowCount: number;
  cache: RowCache;
  /** Rows drawn above the result's rows, e.g. new rows of the data editor; their cells
   *  come from `decorate`. Row indices of the grid count them first. */
  topRows?: number;
  /** Bumped whenever the cache receives rows. */
  version: number;
  /** Server-side sort shown in the header (data viewer). */
  sort?: { column: string; desc: boolean } | null;
  /** Makes headers clickable, e.g. to sort. */
  onHeaderClick?: (column: number) => void;
  source?: GridSource | null;
  /** Called for every visible cell on each frame: keep it cheap. */
  decorate?: (row: number, col: number) => CellDecoration | undefined;
  /** Positioned over the grid, e.g. an inline editor placed with `GridApi.cellRect`. */
  overlay?: ReactNode;
  onCellDoubleClick?: (cell: Cell) => void;
  /** Called when the grid scrolls, e.g. to close an overlay placed over a cell. */
  onScroll?: () => void;
  /** Called when the focused cell changes, e.g. for the value panel. */
  onFocusChange?: (cell: Cell | null) => void;
  /** Called when the selection changes, e.g. for its totals. */
  onSelectionChange?: () => void;
  /** Order, hidden, pinned columns and widths; dragging headers and resizing change it
   *  through `onLayoutChange`, without which the columns keep the result's order. */
  layout?: ColumnLayout;
  onLayoutChange?: (layout: ColumnLayout) => void;
  ref?: Ref<GridApi>;
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
  const rowHeight = Number(v("--grid-row-h"));
  if (rowHeight >= 16) ROW_H = rowHeight;
  const mono = v("--font-mono") || MONO_FALLBACK;
  FONT = `12px ${mono}`;
  FONT_BOLD = `600 12px ${mono}`;
  FONT_SMALL = `11px ${mono}`;
  FONT_NULL = `italic 12px ${mono}`;
  // Canvas text does not load a font face by itself.
  void document.fonts.load(FONT).catch(() => {});
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

/** A header being dragged: from display position `from`, dropped before position `before`. */
interface Drag {
  from: number;
  startX: number;
  moved: boolean;
  before: number;
}

export function DataGrid({
  columns,
  rowCount,
  cache,
  topRows = 0,
  version,
  sort = null,
  onHeaderClick,
  source = null,
  decorate,
  overlay,
  onCellDoubleClick,
  onScroll,
  onFocusChange,
  onSelectionChange,
  layout,
  onLayoutChange,
  ref,
}: Props) {
  const outerRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const spacerRef = useRef<HTMLDivElement>(null);
  /** By result column, so they follow the columns when the layout moves them. */
  const widths = useRef<number[]>([]);
  const sizedToData = useRef(false);
  const anchor = useRef<Cell | null>(null);
  const focus = useRef<Cell | null>(null);
  const resizing = useRef<{ col: number; startX: number; startW: number } | null>(null);
  const dragging = useRef<Drag | null>(null);
  const selecting = useRef(false);
  const theme = useRef<Theme | null>(null);
  const frame = useRef(0);
  const shown = layout ?? EMPTY_LAYOUT;
  const resolved = useMemo(() => resolveLayout(columns, shown), [columns, shown]);
  // Latest props for the imperative draw loop.
  const latest = {
    columns,
    rowCount,
    topRows,
    cache,
    sort,
    onHeaderClick,
    decorate,
    onFocusChange,
    onSelectionChange,
    layout: shown,
    onLayoutChange,
    resolved,
  };
  const props = useRef(latest);
  props.current = latest;
  /** Rows shown: the top rows, then the result's. */
  const total = () => props.current.rowCount + props.current.topRows;
  const [menu, setMenu] = useState<MenuState | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);

  const measure = useCallback((text: string, font: string) => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!ctx) return text.length * 7;
    ctx.font = font;
    return ctx.measureText(text).width;
  }, []);

  // Header-based widths until the first rows arrive, then sized to fit them (see `draw`).
  useLayoutEffect(() => {
    widths.current = columns.map((c) => props.current.layout.widths[c.name] ?? headerWidth(c));
    sizedToData.current = false;
    anchor.current = focus.current = null;
    props.current.onFocusChange?.(null);
    props.current.onSelectionChange?.();
    // headerWidth only depends on measure.
  }, [columns, measure]);

  // Saved widths win; a layout without widths (a reset) sizes the columns again.
  const savedWidths = shown.widths;
  const hadWidths = useRef(Object.keys(savedWidths).length > 0);
  useLayoutEffect(() => {
    const any = Object.keys(savedWidths).length > 0;
    if (!any && hadWidths.current) {
      widths.current = columns.map((c) => headerWidth(c));
      sizedToData.current = false;
    }
    hadWidths.current = any;
    columns.forEach((c, i) => {
      const w = savedWidths[c.name];
      if (w !== undefined) widths.current[i] = w;
    });
    schedule();
  }, [savedWidths]);

  function headerWidth(c: ColumnInfo) {
    return Math.max(MIN_COL_W, measure(c.name, FONT_BOLD), measure(c.db_type, FONT_SMALL)) + PAD * 2;
  }

  /** Widens columns to fit the first rows, once per result; saved widths stay. */
  function sizeToData() {
    const { columns, rowCount, cache, layout } = props.current;
    if (sizedToData.current || rowCount === 0 || cache.get(0, 0) === undefined) return;
    sizedToData.current = true;
    const canvas = canvasRef.current;
    if (canvas) {
      canvas.dataset.measuredWith = document.fonts.check(FONT) ? FONT : "fallback";
      canvas.dataset.measures = String(Number(canvas.dataset.measures ?? 0) + 1);
    }
    const sample = Math.min(rowCount, SIZE_SAMPLE_ROWS);
    widths.current = columns.map((c, col) => {
      const saved = layout.widths[c.name];
      if (saved !== undefined) return saved;
      let max = headerWidth(c);
      for (let r = 0; r < sample; r++) {
        const v = cache.get(r, col);
        if (v) max = Math.max(max, measure(v, FONT) + PAD * 2);
      }
      return Math.min(MAX_INITIAL_COL_W, Math.ceil(max));
    });
  }

  const gutterWidth = () => Math.ceil(measure(String(total()), FONT)) + PAD * 2;

  const contentHeight = () => HEADER_H + total() * ROW_H;

  /** What a cell shows: its decoration's text, else the fetched value. */
  const cellText = (row: number, col: number, d: CellDecoration | undefined) => {
    if (d?.text !== undefined) return d.text;
    const top = props.current.topRows;
    return row < top ? "" : props.current.cache.get(row - top, col);
  };

  /** Maps the DOM scroll position to a logical one, for results taller than the browser allows. */
  const logicalScrollTop = () => {
    const el = scrollRef.current;
    if (!el) return 0;
    const real = contentHeight();
    const dom = Math.min(real, MAX_SCROLL_PX);
    if (real === dom || dom <= el.clientHeight) return el.scrollTop;
    return (el.scrollTop * (real - el.clientHeight)) / (dom - el.clientHeight);
  };

  const width = (col: number) => widths.current[col] ?? MIN_COL_W;
  /** Display position of a result column; `-1` when hidden. */
  const position = (col: number) => props.current.resolved.position[col] ?? -1;

  /** Distance from the first column to display position `d`. */
  const offsetX = (d: number) => {
    const { view } = props.current.resolved;
    let x = 0;
    for (let i = 0; i < d; i++) x += width(view[i]!);
    return x;
  };

  /** Where display position `d` starts on screen, and the left edge it is clipped to:
   *  pinned columns never scroll, the others pass under them. */
  const placeX = (d: number) => {
    const gutter = gutterWidth();
    const { pinned } = props.current.resolved;
    const pinnedWidth = offsetX(pinned);
    const sx = scrollRef.current?.scrollLeft ?? 0;
    return d < pinned
      ? { x: gutter + offsetX(d), edge: gutter }
      : { x: gutter + offsetX(d) - sx, edge: gutter + pinnedWidth };
  };

  /** The selection's rows and display positions. */
  function selectionRect() {
    const a = anchor.current;
    const f = focus.current;
    if (!a || !f) return null;
    const df = position(f.col);
    if (df < 0) return null;
    const da = position(a.col) < 0 ? df : position(a.col);
    return {
      r0: Math.min(a.row, f.row),
      r1: Math.max(a.row, f.row),
      d0: Math.min(da, df),
      d1: Math.max(da, df),
    };
  }

  const draw = useCallback(() => {
    frame.current = 0;
    const canvas = canvasRef.current;
    const scroller = scrollRef.current;
    const outer = outerRef.current;
    if (!canvas || !scroller || !outer) return;
    const { columns, cache, decorate, topRows, resolved } = props.current;
    const { view, pinned } = resolved;
    const rows = total();
    const t = (theme.current ??= readTheme(outer));
    canvas.dataset.font = FONT;
    sizeToData();
    // Column resizes change the scrollable width without a React render.
    const contentWidth = gutterWidth() + offsetX(view.length) + 200;
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
    const lastRow = Math.min(rows - 1, Math.floor((sy + h - HEADER_H) / ROW_H));
    const rowY = (r: number) => HEADER_H + r * ROW_H - sy;
    const pinnedWidth = offsetX(pinned);

    const sel = selectionRect();

    // Row backgrounds.
    for (let r = firstRow; r <= lastRow; r++) {
      if (r % 2 === 1) {
        ctx.fillStyle = t.bgAlt;
        ctx.fillRect(gutter, rowY(r), w - gutter, ROW_H);
      }
    }

    /** One column's cells at `x`, clipped to the right of `edge`. */
    const drawCells = (d: number, x: number, edge: number) => {
      const c = view[d]!;
      const cw = width(c);
      const column = columns[c];
      ctx.save();
      ctx.beginPath();
      ctx.rect(Math.max(x, edge), HEADER_H, cw - (x < edge ? edge - x : 0), h - HEADER_H);
      ctx.clip();
      if (d < pinned) {
        // Opaque, so scrolled columns do not show through.
        ctx.fillStyle = t.bg;
        ctx.fillRect(x, HEADER_H, cw, h - HEADER_H);
        for (let r = firstRow; r <= lastRow; r++) {
          if (r % 2 === 1) {
            ctx.fillStyle = t.bgAlt;
            ctx.fillRect(x, rowY(r), cw, ROW_H);
          }
        }
      }
      const decorations: (CellDecoration | undefined)[] = [];
      if (decorate) {
        for (let r = firstRow; r <= lastRow; r++) {
          const dec = decorate(r, c);
          decorations[r - firstRow] = dec;
          if (dec?.background) {
            ctx.fillStyle = dec.background;
            ctx.fillRect(x, rowY(r), cw, ROW_H);
          }
        }
      }
      if (sel && d >= sel.d0 && d <= sel.d1) {
        const y0 = Math.max(rowY(sel.r0), HEADER_H);
        const y1 = Math.min(rowY(sel.r1 + 1), h);
        if (y1 > y0) {
          ctx.fillStyle = t.selection;
          ctx.fillRect(x, y0, cw, y1 - y0);
        }
      }
      for (let r = firstRow; r <= lastRow; r++) {
        const y = rowY(r) + ROW_H / 2;
        const dec = decorations[r - firstRow];
        const value = cellText(r, c, dec);
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
        ctx.fillStyle = dec?.color ?? t.fg;
        // A hint follows the value, so both start on the left.
        const right = !!column?.numeric && !dec?.hint;
        ctx.textAlign = right ? "right" : "left";
        ctx.fillText(value, right ? x + cw - PAD : x + PAD, y);
        if (dec?.strike) {
          const tw = Math.min(ctx.measureText(value).width, cw - PAD * 2);
          ctx.fillRect(right ? x + cw - PAD - tw : x + PAD, y, tw, 1);
        }
        if (dec?.hint) {
          ctx.fillStyle = t.muted;
          ctx.fillText(dec.hint, x + PAD * 2 + ctx.measureText(value).width, y);
        }
      }
      // Column separator.
      ctx.fillStyle = t.border;
      ctx.fillRect(x + cw - 1, HEADER_H, 1, h - HEADER_H);
      ctx.restore();
    };

    /** One column's header at `x`, clipped to the right of `edge`. */
    const drawHeader = (d: number, x: number, edge: number) => {
      const c = view[d]!;
      const cw = width(c);
      const column = columns[c];
      if (!column) return;
      ctx.save();
      ctx.beginPath();
      ctx.rect(Math.max(x, edge), 0, cw - 1 - (x < edge ? edge - x : 0), HEADER_H);
      ctx.clip();
      ctx.fillStyle = t.header;
      ctx.fillRect(x, 0, cw, HEADER_H);
      ctx.textAlign = "left";
      ctx.font = FONT_BOLD;
      ctx.fillStyle = t.muted;
      ctx.fillText(column.name, x + PAD, 14);
      const sorted = props.current.sort;
      if (sorted?.column === column.name) {
        ctx.fillStyle = t.focus;
        ctx.textAlign = "right";
        ctx.fillText(sorted.desc ? "▼" : "▲", x + cw - PAD, 14);
        ctx.textAlign = "left";
      }
      ctx.font = FONT_SMALL;
      ctx.fillStyle = t.muted;
      ctx.fillText(column.db_type, x + PAD, 29);
      ctx.restore();
      if (x + cw - 1 > edge) {
        ctx.fillStyle = t.border;
        ctx.fillRect(x + cw - 1, 6, 1, HEADER_H - 12);
      }
    };

    /** Every column: scrolled ones first, pinned ones over them. */
    const eachColumn = (paint: (d: number, x: number, edge: number) => void) => {
      let x = gutter + pinnedWidth - sx;
      for (let d = pinned; d < view.length; d++) {
        const cw = width(view[d]!);
        if (x + cw > gutter + pinnedWidth && x < w) paint(d, x, gutter + pinnedWidth);
        x += cw;
      }
      x = gutter;
      for (let d = 0; d < pinned; d++) {
        paint(d, x, gutter);
        x += width(view[d]!);
      }
    };

    eachColumn(drawCells);

    // Focused cell outline.
    const f = focus.current;
    const fd = f ? position(f.col) : -1;
    if (f && fd >= 0 && f.row >= firstRow && f.row <= lastRow) {
      const { x: fx, edge } = placeX(fd);
      const fw = width(f.col);
      if (fx + fw > edge) {
        ctx.strokeStyle = t.focus;
        ctx.lineWidth = 2;
        ctx.strokeRect(Math.max(fx, edge) + 1, rowY(f.row) + 1, fw - 2 - Math.max(0, edge - fx), ROW_H - 2);
      }
    }

    // Row-number gutter, before the header so a partly scrolled-out row slides under it.
    ctx.fillStyle = t.header;
    ctx.fillRect(0, HEADER_H, gutter, h - HEADER_H);
    ctx.font = FONT;
    ctx.textAlign = "right";
    ctx.fillStyle = t.muted;
    for (let r = firstRow; r <= lastRow; r++) {
      ctx.fillText(r < topRows ? "+" : String(r - topRows + 1), gutter - PAD, rowY(r) + ROW_H / 2);
    }

    // Header.
    ctx.fillStyle = t.header;
    ctx.fillRect(0, 0, w, HEADER_H);
    eachColumn(drawHeader);
    ctx.fillStyle = t.border;
    ctx.fillRect(0, HEADER_H - 1, w, 1);
    ctx.fillRect(gutter - 1, 0, 1, h);
    if (pinned > 0) ctx.fillRect(gutter + pinnedWidth - 1, 0, 2, h);

    // Where a dragged header would land.
    const drag = dragging.current;
    if (drag?.moved) {
      const at = drag.before < view.length ? placeX(drag.before).x : placeX(view.length - 1).x + width(view.at(-1)!);
      ctx.fillStyle = t.focus;
      ctx.fillRect(at - 1, 0, 3, h);
    }

    // The cache holds the result's rows only, below the top rows.
    if (lastRow >= topRows) {
      cache.ensure(Math.max(0, firstRow - topRows), lastRow - topRows, props.current.rowCount);
    }
    // Reads only refs, so it never needs to be recreated.
  }, []);

  const schedule = useCallback(() => {
    if (!frame.current) frame.current = requestAnimationFrame(draw);
  }, [draw]);

  const api: GridApi = {
    columns: () => props.current.columns,
    rowCount: total,
    value: (row, col) => cellText(row, col, props.current.decorate?.(row, col)),
    selection: () => {
      const s = selectionRect();
      return s && { r0: s.r0, r1: s.r1, cols: props.current.resolved.view.slice(s.d0, s.d1 + 1) };
    },
    focused: () => focus.current,
    dataRow: (row) => (row < props.current.topRows ? null : row - props.current.topRows),
    select: (a, f = a) => {
      anchor.current = a;
      setFocus(f, true);
    },
    selectAll: () => {
      const { view } = props.current.resolved;
      if (!view.length) return;
      anchor.current = { row: 0, col: view[0]! };
      setFocus({ row: total() - 1, col: view.at(-1)! }, true);
    },
    visibleColumns: () => props.current.resolved.view,
    layout: () => (props.current.onLayoutChange ? props.current.layout : null),
    setLayout: (l) => props.current.onLayoutChange?.(l),
    cellRect: (row, col) => {
      const scroller = scrollRef.current;
      const d = position(col);
      if (!scroller || d < 0) return null;
      const { x: left, edge } = placeX(d);
      const top = HEADER_H + row * ROW_H - logicalScrollTop();
      const w = width(col);
      if (top < HEADER_H || top + ROW_H > scroller.clientHeight || left + w <= edge) return null;
      return { left, top, width: w, height: ROW_H };
    },
    redraw: schedule,
    focus: () => scrollRef.current?.focus(),
  };
  const context = (): GridActionContext => ({ grid: api, source });
  useImperativeHandle(ref, () => api);
  // Decorations come from outside the grid: redraw when they change.
  useEffect(schedule, [schedule, decorate]);

  // Redraw on data, size, layout and theme changes.
  useEffect(schedule, [schedule, version, rowCount, topRows, columns, sort, resolved]);
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
    window.addEventListener(THEME_EVENT, onTheme);
    // Columns measured with a fallback font are sized again once the real one has loaded;
    // widths saved in the layout (resized by hand) stay. Other faces loading later (Inter,
    // an accented letter's subset) change nothing here.
    const onFonts = () => {
      theme.current = readTheme(outer);
      if (canvasRef.current?.dataset.measuredWith !== "fallback" || !document.fonts.check(FONT)) return;
      const { columns, layout } = props.current;
      widths.current = columns.map((c) => layout.widths[c.name] ?? headerWidth(c));
      sizedToData.current = false;
      schedule();
    };
    document.fonts.addEventListener("loadingdone", onFonts);
    return () => {
      observer.disconnect();
      media.removeEventListener("change", onTheme);
      window.removeEventListener(THEME_EVENT, onTheme);
      document.fonts.removeEventListener("loadingdone", onFonts);
      cancelAnimationFrame(frame.current);
      frame.current = 0;
    };
  }, [schedule]);

  /** Grid coordinates under a mouse event: `col` and `border` are result columns, `d` the
   *  display position. */
  function hit(e: React.MouseEvent) {
    const scroller = scrollRef.current!;
    const rect = scroller.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    const gutter = gutterWidth();
    const { view, pinned } = props.current.resolved;
    const pinnedEdge = gutter + offsetX(pinned);
    let col = -1;
    let d = -1;
    let border = -1;
    for (let i = 0; i < view.length; i++) {
      const c = view[i]!;
      const { x, edge } = placeX(i);
      const cw = width(c);
      // Scrolled columns under the pinned ones cannot be hit there.
      if (i >= pinned && px < pinnedEdge) continue;
      if (Math.abs(px - (x + cw)) <= RESIZE_HIT_PX && x + cw > edge) border = c;
      if (px >= Math.max(x, edge) && px < x + cw) {
        col = c;
        d = i;
      }
    }
    const row = Math.floor((py - HEADER_H + logicalScrollTop()) / ROW_H);
    // The scrollbars lie over the cells: a press there drags them and must not sort or select.
    const onScrollbar = px >= scroller.clientWidth || py >= scroller.clientHeight;
    return { px, py, col, d, border, row, inHeader: py < HEADER_H, inGutter: px < gutter, onScrollbar };
  }

  function scrollIntoView(cell: Cell) {
    const scroller = scrollRef.current!;
    const d = position(cell.col);
    const { pinned } = props.current.resolved;
    if (d >= pinned) {
      const pinnedWidth = offsetX(pinned);
      const x = offsetX(d) - pinnedWidth;
      const cw = width(cell.col);
      const viewW = scroller.clientWidth - gutterWidth() - pinnedWidth;
      if (x < scroller.scrollLeft) scroller.scrollLeft = x;
      else if (x + cw > scroller.scrollLeft + viewW) scroller.scrollLeft = x + cw - viewW;
    }

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
    const { view } = props.current.resolved;
    const rowCount = total();
    if (rowCount === 0 || view.length === 0) return;
    const clamped = {
      row: Math.max(0, Math.min(rowCount - 1, cell.row)),
      col: position(cell.col) >= 0 ? cell.col : view[0]!,
    };
    const previous = focus.current;
    focus.current = clamped;
    if (previous?.row !== clamped.row || previous.col !== clamped.col) props.current.onFocusChange?.(clamped);
    if (!extend || !anchor.current) anchor.current = clamped;
    props.current.onSelectionChange?.();
    scrollIntoView(clamped);
    schedule();
  }

  /** The result column `steps` display positions away from `col`, kept in range. */
  function shifted(col: number, steps: number) {
    const { view } = props.current.resolved;
    const d = Math.max(0, Math.min(view.length - 1, position(col) + steps));
    return view[d]!;
  }

  function autosize(col: number) {
    const scroller = scrollRef.current!;
    const { columns } = props.current;
    const first = Math.floor(logicalScrollTop() / ROW_H);
    const visible = Math.ceil(scroller.clientHeight / ROW_H);
    const column = columns[col];
    if (!column) return;
    let max = Math.max(measure(column.name, FONT_BOLD), measure(column.db_type, FONT_SMALL));
    for (let r = first; r < first + visible * 3; r++) {
      const d = props.current.decorate?.(r, col);
      const v = cellText(r, col, d);
      if (v) max = Math.max(max, measure(v, FONT) + (d?.hint ? measure(d.hint, FONT) + PAD : 0));
    }
    widths.current[col] = Math.max(MIN_COL_W, Math.ceil(max) + PAD * 2);
    saveWidth(col);
    schedule();
  }

  /** Keeps a width the user set in the layout, when there is one to keep it in. */
  function saveWidth(col: number) {
    const { layout, onLayoutChange, columns } = props.current;
    const name = columns[col]?.name;
    if (!onLayoutChange || name === undefined) return;
    onLayoutChange({ ...layout, widths: { ...layout.widths, [name]: widths.current[col]! } });
  }

  function onMouseDown(e: React.MouseEvent) {
    if (e.button !== 0) return;
    const h = hit(e);
    if (h.onScrollbar) return;
    if (h.inHeader && h.border >= 0) {
      e.preventDefault();
      resizing.current = { col: h.border, startX: e.clientX, startW: width(h.border) };
      return;
    }
    if (h.inHeader && h.col >= 0 && !h.inGutter) {
      // A click sorts; dragging further moves the column (see onMouseUp).
      dragging.current = { from: h.d, startX: e.clientX, moved: false, before: h.d };
      return;
    }
    if (h.inHeader || h.row < 0 || h.row >= total()) return;
    const { view } = props.current.resolved;
    if (h.inGutter) {
      anchor.current = { row: h.row, col: view[0]! };
      setFocus({ row: h.row, col: view.at(-1)! }, true);
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
    const drag = dragging.current;
    if (drag && e.buttons === 1) {
      if (!drag.moved && Math.abs(e.clientX - drag.startX) < DRAG_PX) return;
      if (!props.current.onLayoutChange) return;
      drag.moved = true;
      const px = hit(e).px;
      const { view } = props.current.resolved;
      // Before the first column whose middle is right of the mouse.
      let before = view.length;
      for (let d = 0; d < view.length; d++) {
        const { x } = placeX(d);
        if (px < x + width(view[d]!) / 2) {
          before = d;
          break;
        }
      }
      drag.before = before;
      scroller.style.cursor = "grabbing";
      schedule();
      return;
    }
    const h = hit(e);
    scroller.style.cursor =
      h.inHeader && h.border >= 0
        ? "col-resize"
        : h.inHeader && h.col >= 0 && props.current.onHeaderClick
          ? "pointer"
          : "default";
    if (selecting.current && e.buttons === 1 && h.col >= 0 && !h.inHeader) {
      setFocus({ row: h.row, col: h.col }, true);
    }
  }

  function onMouseUp() {
    const r = resizing.current;
    if (r) saveWidth(r.col);
    resizing.current = null;
    selecting.current = false;
    const drag = dragging.current;
    dragging.current = null;
    if (!drag) return;
    const { layout, onLayoutChange, columns, resolved } = props.current;
    if (!drag.moved) {
      const col = resolved.view[drag.from];
      if (col !== undefined) props.current.onHeaderClick?.(col);
      return;
    }
    const to = drag.before > drag.from ? drag.before - 1 : drag.before;
    onLayoutChange?.(moveColumn(layout, columns, resolved, drag.from, to));
    schedule();
  }

  function onMouseLeave() {
    const r = resizing.current;
    if (r) saveWidth(r.col);
    resizing.current = null;
    selecting.current = false;
    if (dragging.current) {
      dragging.current = null;
      schedule();
    }
  }

  function onDoubleClick(e: React.MouseEvent) {
    const h = hit(e);
    if (h.onScrollbar) return;
    if (h.inHeader && h.border >= 0) autosize(h.border);
    else if (!h.inHeader && !h.inGutter && h.col >= 0 && h.row >= 0 && h.row < total()) {
      onCellDoubleClick?.({ row: h.row, col: h.col });
    }
  }

  function onContextMenu(e: React.MouseEvent) {
    e.preventDefault();
    const h = hit(e);
    if (h.inHeader && h.col >= 0) {
      // A header's menu acts on its column.
      setFocus({ row: focus.current?.row ?? 0, col: h.col }, false);
    } else if (!h.inHeader && h.row >= 0 && h.row < total() && h.col >= 0) {
      // Right-clicking outside the selection selects the cell under the mouse.
      const sel = selectionRect();
      const inside = sel && h.row >= sel.r0 && h.row <= sel.r1 && h.d >= sel.d0 && h.d <= sel.d1;
      if (!inside) setFocus({ row: h.row, col: h.col }, false);
    }
    setMenu({ x: e.clientX, y: e.clientY, target: { scope: "grid", arg: context() } });
  }

  const showError = (e: unknown) => workbench().showError(toCommandError(e).message);

  function onKeyDown(e: React.KeyboardEvent) {
    const f = focus.current;
    const page = Math.max(1, Math.floor((scrollRef.current!.clientHeight - HEADER_H) / ROW_H) - 1);
    const ctrl = e.ctrlKey || e.metaKey;
    const { view } = props.current.resolved;
    const first = view[0] ?? 0;
    const last = view.at(-1) ?? 0;
    const moves: Record<string, () => Cell | null> = {
      ArrowUp: () => f && { ...f, row: f.row - 1 },
      ArrowDown: () => f && { ...f, row: f.row + 1 },
      ArrowLeft: () => f && { ...f, col: shifted(f.col, -1) },
      ArrowRight: () => f && { ...f, col: shifted(f.col, 1) },
      PageUp: () => f && { ...f, row: f.row - page },
      PageDown: () => f && { ...f, row: f.row + page },
      Home: () => f && (ctrl ? { row: 0, col: first } : { ...f, col: first }),
      End: () => f && (ctrl ? { row: total() - 1, col: last } : { ...f, col: last }),
    };
    const move = moves[e.key];
    if (move) {
      e.preventDefault();
      const next = move() ?? { row: 0, col: first };
      setFocus(next, e.shiftKey);
    } else {
      dispatchKey("grid", context(), e.nativeEvent, showError);
    }
  }

  const spacerHeight = Math.min(HEADER_H + (rowCount + topRows) * ROW_H, MAX_SCROLL_PX);

  return (
    <div className="grid" ref={outerRef}>
      <canvas ref={canvasRef} className="grid-canvas" />
      <div
        ref={scrollRef}
        className="grid-scroll"
        tabIndex={0}
        onScroll={() => {
          schedule();
          onScroll?.();
        }}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onMouseLeave={onMouseLeave}
        onDoubleClick={onDoubleClick}
        onContextMenu={onContextMenu}
        onKeyDown={onKeyDown}
      >
        <div ref={spacerRef} style={{ height: spacerHeight }} />
      </div>
      {overlay}
      {menu && <ContextMenu menu={menu} onClose={closeMenu} onError={showError} />}
    </div>
  );
}
