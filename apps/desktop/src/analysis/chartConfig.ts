/**
 * A result's chart: its type, columns and aggregation, kept with the tab for each statement
 * (`SavedChart`, like the data viewer's sorts), and the first choice made from the columns.
 * Pure: the unit tests import it.
 */
import type { ChartRequest } from "../api";

export type ChartKind = ChartRequest["kind"];
export type Aggregate = ChartRequest["aggregate"];

/** What a chart draws, columns by name so that it survives a run that reorders them. */
export interface ChartConfig {
  kind: ChartKind;
  x: string;
  y: string[];
  aggregate: Aggregate;
}

/** A statement's chart in the tab: shown in place of the grid or not, and what it draws
 *  (`null` until chosen: the first choice is made from the columns). */
export interface SavedChart {
  query: string;
  shown: boolean;
  config: ChartConfig | null;
}

/** A result column as the chart sees it (`ColumnInfo` fits). */
export interface ChartColumn {
  name: string;
  numeric: boolean;
  db_type: string;
}

const KINDS: readonly string[] = ["bar", "line", "scatter", "pie"];
const AGGREGATES: readonly string[] = ["sum", "count", "avg", "min", "max"];
/** Statements whose chart a tab keeps at most. */
const KEPT = 20;

/** The statement a chart belongs to, whatever the spacing around it (as the viewer's sorts). */
export const chartKey = (sql: string) => sql.trim().replace(/;$/, "");

/** Dates and timestamps, by the database's type name. */
export const isTime = (c: ChartColumn) => !c.numeric && /date|timestamp/i.test(c.db_type);

/** The first chart of a result: the first text or date column across, the first number columns
 *  (three at most) up; bars over text, lines over dates and numbers. */
export function autoConfig(columns: ChartColumn[]): ChartConfig {
  const x = columns.find((c) => !c.numeric) ?? columns[0];
  const y = columns
    .filter((c) => c.numeric && c !== x)
    .slice(0, 3)
    .map((c) => c.name);
  const text = !!x && !x.numeric && !isTime(x);
  return { kind: text ? "bar" : "line", x: x?.name ?? "", y, aggregate: y.length > 0 ? "sum" : "count" };
}

/** `config` if every column it names is in the result, else `null`: the user chooses again. */
export function validConfig(config: ChartConfig, columns: ChartColumn[]): ChartConfig | null {
  const names = new Set(columns.map((c) => c.name));
  return names.has(config.x) && config.y.every((n) => names.has(n)) ? config : null;
}

/** What the backend computes for `config`, columns by position; `null` if one is missing. The
 *  X column is not a series too (a chart saved before it was chosen as X). */
export function toRequest(config: ChartConfig, columns: ChartColumn[]): ChartRequest | null {
  const index = (name: string) => columns.findIndex((c) => c.name === name);
  const x = index(config.x);
  const y = config.y.filter((n) => n !== config.x).map(index);
  return x < 0 || y.some((i) => i < 0) ? null : { kind: config.kind, x, y, aggregate: config.aggregate };
}

export function chartOf(charts: readonly SavedChart[] | undefined, sql: string): SavedChart | null {
  const query = chartKey(sql);
  return charts?.find((c) => c.query === query) ?? null;
}

/** `charts` with the chart of `sql` changed by `patch`, created when missing; the oldest go
 *  past `KEPT`. */
export function withChart(
  charts: readonly SavedChart[] | undefined,
  sql: string,
  patch: Partial<Omit<SavedChart, "query">>,
): SavedChart[] {
  const query = chartKey(sql);
  const current = chartOf(charts, sql) ?? { query, shown: false, config: null };
  return [...(charts ?? []).filter((c) => c.query !== query), { ...current, ...patch }].slice(-KEPT);
}

/** The charts saved with a tab, without what an older or damaged save holds. */
export function savedCharts(v: unknown): SavedChart[] {
  if (!Array.isArray(v)) return [];
  return v.flatMap((c: unknown) => {
    if (!c || typeof c !== "object") return [];
    const { query, shown, config } = c as Record<string, unknown>;
    return typeof query === "string" ? [{ query, shown: shown === true, config: parseConfig(config) }] : [];
  });
}

function parseConfig(v: unknown): ChartConfig | null {
  if (!v || typeof v !== "object") return null;
  const { kind, x, y, aggregate } = v as Record<string, unknown>;
  const names = Array.isArray(y) && y.every((n) => typeof n === "string");
  if (typeof kind !== "string" || !KINDS.includes(kind) || typeof x !== "string" || !names) return null;
  if (typeof aggregate !== "string" || !AGGREGATES.includes(aggregate)) return null;
  return { kind: kind as ChartKind, x, y: y as string[], aggregate: aggregate as Aggregate };
}
