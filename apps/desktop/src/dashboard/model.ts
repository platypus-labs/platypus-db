/**
 * A dashboard: a grid of widgets, each showing the result of a saved script as a table, a
 * chart or a single value. This is the pure part (the unit tests import it): the widgets and
 * what a tab and a file keep of them, the checks that keep a dashboard read-only, and the
 * rules that pick the connection and the row limit of a widget. Running them is `run.ts`.
 */
import type { ScriptEntry } from "../api";
import type { ChartConfig } from "../analysis/chartConfig";

export type WidgetView = "table" | "chart" | "value";

/** One cell of the grid. */
export interface Widget {
  id: string;
  title: string;
  /** The script it shows, by its path in the project's scripts. */
  script: string;
  /** The connection it runs on; `null` takes the script's own link, then the dashboard's. */
  connection: string | null;
  view: WidgetView;
  /** Table: the rows shown (the first ones). */
  rows: number;
  /** Chart: what it draws, `null` until chosen (the first choice is made from the columns). */
  chart: ChartConfig | null;
  /** Columns of the grid it takes, 1 to 3. */
  width: number;
  /** Rows of the grid it takes, 1 or 2. */
  height: number;
}

/** What a dashboard keeps, in a tab and in its file. */
export interface DashboardContent {
  widgets: Widget[];
  /** Minutes between two refreshes while the tab is shown; 0 is off. */
  refresh: number;
  /** The connection the widgets run on when neither they nor their script name one. */
  connection: string | null;
}

export const VIEWS: readonly WidgetView[] = ["table", "chart", "value"];
/** Auto-refresh choices, in minutes. */
export const REFRESH_CHOICES: readonly number[] = [0, 1, 5, 15, 60];
export const DEFAULT_ROWS = 100;
const MAX_ROWS = 10_000;
/** Rows a chart is drawn from: more would not show, and cost the memory of every refresh. */
export const CHART_ROWS = 5000;
export const MAX_WIDTH = 3;
export const MAX_HEIGHT = 2;

let next = 0;
const newWidgetId = () => `${Date.now().toString(36)}-${(next++).toString(36)}`;

const clamp = (n: number, min: number, max: number) => Math.min(max, Math.max(min, n));

/** A widget with the defaults below `init`. */
export function newWidget(init: Partial<Widget> = {}): Widget {
  return {
    id: newWidgetId(),
    title: "",
    script: "",
    connection: null,
    view: "table",
    rows: DEFAULT_ROWS,
    chart: null,
    width: 1,
    height: 1,
    ...init,
  };
}

/** The most rows the widget reads from its script: what its view can show. */
export function rowLimit(widget: Pick<Widget, "view" | "rows">): number {
  if (widget.view === "value") return 1;
  if (widget.view === "chart") return CHART_ROWS;
  return clamp(Math.round(widget.rows) || DEFAULT_ROWS, 1, MAX_ROWS);
}

/** What a widget is called: its title, else its script's name. */
export function widgetTitle(widget: Pick<Widget, "title" | "script">): string {
  const title = widget.title.trim();
  if (title) return title;
  return widget.script.slice(widget.script.lastIndexOf("/") + 1).replace(/\.sql$/i, "");
}

/** The connection a widget runs on: its own choice, else the script's link, else the dashboard's. */
export function connectionOf(widget: Pick<Widget, "connection">, entry: Pick<ScriptEntry, "connection"> | null, dashboard: string | null): string | null {
  return widget.connection ?? entry?.connection ?? dashboard;
}

/** The script file `path` among the project's `entries`, or `undefined` if it was renamed or
 *  deleted (a folder of that name is no script). */
export function findScript(entries: readonly ScriptEntry[], path: string): ScriptEntry | undefined {
  return entries.find((e) => !e.folder && e.path === path);
}

/** The widgets with `id` moved by `by` places; the same array when it cannot move. */
export function moveWidget(widgets: readonly Widget[], id: string, by: -1 | 1): Widget[] {
  const i = widgets.findIndex((w) => w.id === id);
  const j = i + by;
  if (i < 0 || j < 0 || j >= widgets.length) return [...widgets];
  const moved = [...widgets];
  [moved[i], moved[j]] = [moved[j]!, moved[i]!];
  return moved;
}

/** What a statement shows the checks: the part of `StatementSpan` they read. */
export interface StatementFacts {
  read_only: boolean;
  kind: string;
  parameters: readonly unknown[];
}

/** Why a script cannot feed a widget, or `null` when it can: it must hold statements that
 *  only read (as the editor tells for a read-only connection), no client commands, and no
 *  parameters, since nobody is there to give them a value. */
export type Refusal = "empty" | "changes" | "command" | "parameters";

export function refusal(statements: readonly StatementFacts[]): Refusal | null {
  if (statements.length === 0) return "empty";
  if (statements.some((s) => s.kind === "command")) return "command";
  if (statements.some((s) => !s.read_only)) return "changes";
  if (statements.some((s) => s.parameters.length > 0)) return "parameters";
  return null;
}

/** The first cell of a single-value widget as it is written: SQL NULL as `NULL`. */
export const valueText = (cell: string | null | undefined): string => (cell == null ? "NULL" : cell);

const FORMAT = "platypus-dashboard";
const VERSION = 1;

/** The dashboard as the file in the data folder keeps it: JSON. */
export function dashboardFile(title: string, content: DashboardContent): string {
  return `${JSON.stringify({ format: FORMAT, version: VERSION, title, ...content }, null, 2)}\n`;
}

const text = (v: unknown) => (typeof v === "string" ? v : null);

/** Reads a chart's configuration (`parseConfig`), `null` for one that is not valid. */
export type ChartReader = (v: unknown) => ChartConfig | null;

function readWidget(v: unknown, chart: ChartReader): Widget | null {
  if (!v || typeof v !== "object") return null;
  const w = v as Record<string, unknown>;
  const script = text(w.script);
  if (script === null || script === "") return null;
  const num = (value: unknown, fallback: number) => (typeof value === "number" && Number.isFinite(value) ? value : fallback);
  return newWidget({
    ...(typeof w.id === "string" && w.id ? { id: w.id } : {}),
    title: text(w.title) ?? "",
    script,
    connection: text(w.connection),
    view: VIEWS.find((view) => view === w.view) ?? "table",
    rows: clamp(Math.round(num(w.rows, DEFAULT_ROWS)), 1, MAX_ROWS),
    chart: chart(w.chart),
    width: clamp(Math.round(num(w.width, 1)), 1, MAX_WIDTH),
    height: clamp(Math.round(num(w.height, 1)), 1, MAX_HEIGHT),
  });
}

/** The widgets and settings of `value` (a file, or what a tab saved), whatever it lacks or
 *  holds wrongly; `chart` reads a widget's chart configuration. */
export function readContent(value: unknown, chart: ChartReader): DashboardContent {
  const v = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const widgets = Array.isArray(v.widgets) ? v.widgets.flatMap((w: unknown) => readWidget(w, chart) ?? []) : [];
  const refresh = typeof v.refresh === "number" && REFRESH_CHOICES.includes(v.refresh) ? v.refresh : 0;
  return { widgets, refresh, connection: text(v.connection) };
}

/** Reads a dashboard file's text: its title and content; throws on another kind of file. */
export function parseDashboardFile(source: string, chart: ChartReader): { title: string; content: DashboardContent } {
  let value: unknown;
  try {
    value = JSON.parse(source);
  } catch {
    throw new Error("not a dashboard file");
  }
  if (!value || typeof value !== "object" || (value as { format?: unknown }).format !== FORMAT) {
    throw new Error("not a dashboard file");
  }
  return { title: text((value as { title?: unknown }).title) ?? "", content: readContent(value, chart) };
}
