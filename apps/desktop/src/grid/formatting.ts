/**
 * Conditional formatting of the grid: rules that color a cell, or its whole row, from the
 * value of a column (equal to, containing, in a range, NULL, or a color scale over the
 * numbers). Rules name their column, so a table's rules still apply when the query around it
 * changes; a rule whose column the result does not have is ignored. The first rule that
 * matches colors the cell.
 */
import type { ColumnInfo } from "../api";
import type { CellDecoration } from "./DataGrid";

export type Condition =
  | { kind: "equals"; value: string }
  | { kind: "not_equals"; value: string }
  | { kind: "contains"; value: string }
  /** Numbers from `min` to `max`, both included; an empty bound is open. */
  | { kind: "between"; min: string; max: string }
  | { kind: "null" }
  | { kind: "not_null" }
  /** Numbers colored from `color` (the lowest) to `to` (the highest). */
  | { kind: "scale"; to: ColorName };

export type ConditionKind = Condition["kind"];

export const COLOR_NAMES = ["red", "orange", "yellow", "green", "teal", "blue", "purple", "gray"] as const;
export type ColorName = (typeof COLOR_NAMES)[number];

export interface FormatRule {
  id: string;
  column: string;
  condition: Condition;
  color: ColorName;
  /** The whole row, or only the column's cell. */
  row: boolean;
}

/** Translucent, so the text and the selection stay readable in light and dark themes. */
const RGB: Record<ColorName, [number, number, number]> = {
  red: [224, 49, 49],
  orange: [247, 103, 7],
  yellow: [250, 176, 5],
  green: [47, 158, 68],
  teal: [12, 166, 120],
  blue: [28, 126, 214],
  purple: [132, 94, 247],
  gray: [134, 142, 150],
};
const ALPHA = 0.28;

/** A color as the grid paints it under a cell's text. */
export function colorOf(name: ColorName, alpha = ALPHA): string {
  const [r, g, b] = RGB[name];
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/** The color of a scale at `t` (0 the lowest, 1 the highest). */
export function scaleColor(from: ColorName, to: ColorName, t: number): string {
  const a = RGB[from];
  const b = RGB[to];
  const k = Math.min(1, Math.max(0, t));
  const mix = (i: number) => Math.round(a[i]! + (b[i]! - a[i]!) * k);
  return `rgba(${mix(0)}, ${mix(1)}, ${mix(2)}, ${ALPHA})`;
}

const DECIMAL = /^[+-]?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?$/i;

/** A decimal number written as plain text (`-12.5`, `1e3`), else `null`: not `0x10`. */
export function parseNumber(text: string): number | null {
  const trimmed = text.trim();
  if (!DECIMAL.test(trimmed)) return null;
  const n = Number(trimmed);
  return Number.isFinite(n) ? n : null;
}

/** Reads a value of a column as a number, or `null`. */
type NumberOf = (text: string) => number | null;

/** The lowest and highest number of a column, for its color scales. */
export type Ranges = Map<number, [number, number]>;

/** Whether a value meets a condition; scales always do, when the value is a number. */
export function matches(condition: Condition, value: string | null, number: NumberOf): boolean {
  switch (condition.kind) {
    case "null":
      return value === null;
    case "not_null":
      return value !== null;
    case "equals":
      return value !== null && sameText(value, condition.value, number);
    case "not_equals":
      return value === null || !sameText(value, condition.value, number);
    case "contains":
      return value !== null && value.toLowerCase().includes(condition.value.toLowerCase());
    case "between": {
      const n = value === null ? null : number(value);
      if (n === null) return false;
      const min = parseNumber(condition.min);
      const max = parseNumber(condition.max);
      if (min === null && max === null) return false;
      return (min === null || n >= min) && (max === null || n <= max);
    }
    case "scale":
      return value !== null && number(value) !== null;
  }
}

/** Equal as numbers when both are, else as text ignoring case: `1.50` is `1.5`. */
function sameText(value: string, wanted: string, number: NumberOf): boolean {
  const a = number(value);
  const b = parseNumber(wanted);
  if (a !== null && b !== null) return a === b;
  return value.trim().toLowerCase() === wanted.trim().toLowerCase();
}

/** Rules resolved against a result: each with the result column it reads, and how. */
interface ResolvedRule {
  rule: FormatRule;
  col: number;
  number: NumberOf;
}

/** `plain` turns a number as the grid shows it (`1.234,5`) back to plain text: only number
 *  columns are written that way, so text columns are read as they are. */
export function resolveRules(rules: FormatRule[], columns: ColumnInfo[], plain: (text: string) => string): ResolvedRule[] {
  const out: ResolvedRule[] = [];
  for (const rule of rules) {
    const col = columns.findIndex((c) => c.name === rule.column);
    if (col < 0) continue;
    const number: NumberOf = columns[col]!.numeric ? (text) => parseNumber(plain(text)) : parseNumber;
    out.push({ rule, col, number });
  }
  return out;
}

/** The result columns whose color scales need their lowest and highest numbers. */
export function scaledColumns(rules: ResolvedRule[]): number[] {
  return [...new Set(rules.filter((r) => r.rule.condition.kind === "scale").map((r) => r.col))];
}

/**
 * The background of cell `col` of a row, from the first rule that matches: a row rule
 * colors every cell of its row, a cell rule only its column's. `value(c)` reads the row's
 * value of column `c`; `undefined` while it is not loaded.
 */
export function formatCell(
  rules: ResolvedRule[],
  col: number,
  value: (col: number) => string | null | undefined,
  ranges: Ranges,
): CellDecoration | undefined {
  for (const { rule, col: read, number } of rules) {
    if (!rule.row && read !== col) continue;
    const v = value(read);
    if (v === undefined || !matches(rule.condition, v, number)) continue;
    if (rule.condition.kind === "scale") {
      const range = ranges.get(read);
      const n = number(v!);
      if (!range || n === null) continue;
      const [low, high] = range;
      const t = high > low ? (n - low) / (high - low) : 0.5;
      return { background: scaleColor(rule.color, rule.condition.to, t) };
    }
    return { background: colorOf(rule.color) };
  }
  return undefined;
}

/** A condition of `kind` with the values of `previous` kept where they fit. */
export function conditionOf(kind: ConditionKind, previous?: Condition): Condition {
  const value = previous && "value" in previous ? previous.value : "";
  switch (kind) {
    case "equals":
    case "not_equals":
    case "contains":
      return { kind, value };
    case "between":
      return previous?.kind === "between" ? previous : { kind, min: "", max: "" };
    case "null":
    case "not_null":
      return { kind };
    case "scale":
      return previous?.kind === "scale" ? previous : { kind, to: "red" };
  }
}

/** Whether a rule can be saved: a value to compare with, a bound for a range. */
export function complete(condition: Condition): boolean {
  switch (condition.kind) {
    case "equals":
    case "not_equals":
    case "contains":
      return condition.value.trim() !== "";
    case "between": {
      const min = condition.min.trim();
      const max = condition.max.trim();
      if (min === "" && max === "") return false;
      return (min === "" || parseNumber(min) !== null) && (max === "" || parseNumber(max) !== null);
    }
    default:
      return true;
  }
}

/** Moves the rule at `from` one place up (`-1`) or down (`1`). */
export function moveRule(rules: FormatRule[], from: number, by: -1 | 1): FormatRule[] {
  const to = from + by;
  if (to < 0 || to >= rules.length) return rules;
  const out = [...rules];
  [out[from], out[to]] = [out[to]!, out[from]!];
  return out;
}

/** Rules of data viewer tables, kept in this browser profile like their column layouts. */
const KEY = "platypus.formatting.v1";

function stored(): Record<string, FormatRule[]> {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) ?? "{}") as unknown;
    return parsed && typeof parsed === "object" ? (parsed as Record<string, FormatRule[]>) : {};
  } catch {
    return {};
  }
}

export function loadRules(key: string): FormatRule[] {
  const rules = stored()[key];
  return Array.isArray(rules) ? rules.filter(valid) : [];
}

export function saveRules(key: string, rules: FormatRule[]) {
  try {
    const all = stored();
    if (rules.length === 0) delete all[key];
    else all[key] = rules;
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    // Storage can be unavailable or full; rules are a convenience.
  }
}

/** A stored rule this version can read, every field of its condition included: the grid
 *  reads them on each frame. */
export function valid(rule: FormatRule): boolean {
  if (typeof rule !== "object" || rule === null) return false;
  const color = (c: unknown) => (COLOR_NAMES as readonly unknown[]).includes(c);
  const text = (t: unknown) => typeof t === "string";
  const c = rule.condition as Partial<Record<string, unknown>> | undefined;
  if (!text(rule.id) || !text(rule.column) || !color(rule.color) || typeof rule.row !== "boolean" || !c) return false;
  switch (c.kind) {
    case "equals":
    case "not_equals":
    case "contains":
      return text(c.value);
    case "between":
      return text(c.min) && text(c.max);
    case "null":
    case "not_null":
      return true;
    case "scale":
      return color(c.to);
    default:
      return false;
  }
}
