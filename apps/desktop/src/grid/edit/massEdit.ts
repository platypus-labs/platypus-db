/**
 * Mass edit of the data editor's selection: one value for every cell, fill down, find and
 * replace, number sequences, and the checks by column type they share with the cell editor.
 * Pure, so a selection of thousands of cells is computed in one pass, away from React, and
 * tested on its own. The results are `CellChange`s: pending changes like typed ones.
 *
 * The checks mirror what saving accepts (`platypus-sql/src/dml.rs`), so a value that cannot
 * be saved is reported before it becomes a change.
 */
import type { CellInput, ColumnInfo, ColumnMeta } from "../../api";
import type { CellChange, EditState } from "./state";

/** What a column takes, as far as the checks go. */
export type ValueKind = "integer" | "number" | "bool" | "binary" | "json" | "enum" | "text" | "other";

export interface ColumnRule {
  kind: ValueKind;
  nullable: boolean;
  enumValues: readonly string[];
  /** Floating-point columns also take NaN and the infinities. */
  float: boolean;
}

/** Why a cell was left as it was; the UI words it. */
export type Problem =
  | "integer"
  | "number"
  | "bool"
  | "binary"
  | "json"
  | "enum"
  | "null"
  /** The result column is not one of the table's (an expression). */
  | "readonly"
  /** A number sequence in a column that does not hold numbers. */
  | "not_number";

/** Cells of one column left as they were, for one reason. */
export interface Rejection {
  col: number;
  problem: Problem;
  cells: number;
}

export interface Outcome {
  changes: CellChange[];
  rejected: Rejection[];
}

/** A selected cell, as mass edits read it. */
export interface SelectedCell {
  gridRow: number;
  col: number;
  /** What the cell holds now: its pending value, else the loaded one. */
  current: CellInput;
  /** The loaded value of a result row, so setting it back drops the change. */
  original?: string | null;
}

/** The writable column's rule, `undefined` for columns that are not the table's. */
export type Rules = (col: number) => ColumnRule | undefined;

const JSON_TYPE = /json/i;
const BOOL_TYPE = /^bool(ean)?$/i;
const INTEGER_TYPE = /\b(tiny|small|medium|big)?int(eger)?\d*\b|\b(small|big)?serial\d*\b|\bu?int(8|16|32|64|128)\b/i;
const NUMBER_TYPE = /\b(numeric|decimal|dec|real|double|float\d*|number|money|smallmoney)\b/i;
const FLOAT_TYPE = /\b(real|double|float\d*)\b/i;
const TEXT_TYPE = /char|text|clob|string|xml|\bname\b/i;
const INTEGER = /^[+-]?\d+$/;
const NUMBER = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;
const FLOAT_WORDS = new Set(["nan", "infinity", "+infinity", "-infinity", "inf", "-inf"]);
const BOOL_WORDS = new Set(["true", "t", "1", "yes", "y", "on", "false", "f", "0", "no", "n", "off"]);
const HEX = /^(\\x|0x)?([0-9a-fA-F]{2})*$/;

/** The rule of a result column, from its type and the table's catalog. */
export function columnRule(column: ColumnInfo, meta: ColumnMeta | undefined): ColumnRule {
  const type = meta?.data_type ?? column.db_type;
  const rule = (kind: ValueKind): ColumnRule => ({
    kind,
    nullable: meta?.nullable ?? true,
    enumValues: meta?.enum_values ?? [],
    float: FLOAT_TYPE.test(type),
  });
  if (meta?.enum_values.length) return rule("enum");
  // Arrays and composite values are literals of their own.
  if (type.endsWith("[]") || meta?.fields.length) return rule("other");
  if (column.binary) return rule("binary");
  // Booleans stored as numbers (SQLite, MySQL TINYINT) take 1 and 0, like the cell editor.
  if (BOOL_TYPE.test(type) && !column.numeric) return rule("bool");
  if (JSON_TYPE.test(type)) return rule("json");
  if (INTEGER_TYPE.test(type)) return rule("integer");
  if (column.numeric || NUMBER_TYPE.test(type)) return rule("number");
  if (TEXT_TYPE.test(type)) return rule("text");
  return rule("other");
}

export const isNumberRule = (rule: ColumnRule | undefined) => rule?.kind === "integer" || rule?.kind === "number";

/** Columns whose cells can be loaded from a file: binary ones as bytes, text ones as UTF-8. */
export const loadsFromFile = (rule: ColumnRule | undefined) =>
  rule?.kind === "binary" || rule?.kind === "text" || rule?.kind === "json";

/** An empty value in a number column is NULL, as in the cell editor. */
export function normalize(rule: ColumnRule, input: CellInput): CellInput {
  return isNumberRule(rule) && input.kind === "value" && input.text.trim() === "" ? { kind: "null" } : input;
}

function validJson(text: string): boolean {
  if (text.trim() === "") return true;
  try {
    JSON.parse(text);
    return true;
  } catch {
    return false;
  }
}

/** Why `input` does not fit a column with `rule`; `null` when it does. */
export function checkValue(rule: ColumnRule, input: CellInput): Problem | null {
  if (input.kind === "default") return null;
  if (input.kind === "null") return rule.nullable ? null : "null";
  const text = input.text.trim();
  switch (rule.kind) {
    case "integer":
      return INTEGER.test(text) ? null : "integer";
    case "number":
      return NUMBER.test(text) || (rule.float && FLOAT_WORDS.has(text.toLowerCase())) ? null : "number";
    case "bool":
      return BOOL_WORDS.has(text.toLowerCase()) ? null : "bool";
    case "binary":
      return HEX.test(text) ? null : "binary";
    case "json":
      return validJson(input.text) ? null : "json";
    case "enum":
      return rule.enumValues.includes(input.text) ? null : "enum";
    default:
      return null;
  }
}

/** Gathers rejected cells by column and reason. */
class Collector {
  readonly changes: CellChange[] = [];
  private readonly rejections = new Map<string, Rejection>();

  reject(col: number, problem: Problem) {
    const key = `${col}:${problem}`;
    const r = this.rejections.get(key);
    if (r) r.cells++;
    else this.rejections.set(key, { col, problem, cells: 1 });
  }

  /** Changes `cell` to `value`, unless the column cannot take it. */
  put(cell: SelectedCell, rule: ColumnRule | undefined, value: CellInput) {
    if (!rule) return this.reject(cell.col, "readonly");
    const problem = checkValue(rule, value);
    if (problem) return this.reject(cell.col, problem);
    this.changes.push({ gridRow: cell.gridRow, col: cell.col, value, original: cell.original });
  }

  outcome(): Outcome {
    return { changes: this.changes, rejected: [...this.rejections.values()] };
  }
}

/**
 * The selected cells of rows `r0..r1` and columns `cols`, deleted rows left out. `loaded` are
 * the full values of the result rows from `firstLoaded` on (`api.rowValues`).
 */
export function selectionCells(
  state: EditState,
  selection: { r0: number; r1: number; cols: readonly number[] },
  loaded: readonly (readonly (string | null)[])[],
  firstLoaded: number,
): SelectedCell[] {
  const n = state.inserts.length;
  const out: SelectedCell[] = [];
  for (let r = selection.r0; r <= selection.r1; r++) {
    if (r < n) {
      // Columns a new row leaves out take their default.
      const row = state.inserts[r]!;
      for (const col of selection.cols) out.push({ gridRow: r, col, current: row.get(col) ?? { kind: "default" } });
      continue;
    }
    const row = r - n;
    if (state.deletes.has(row)) continue;
    const values = loaded[row - firstLoaded] ?? [];
    const pending = state.updates.get(row);
    for (const col of selection.cols) {
      const original = values[col] ?? null;
      const current: CellInput = pending?.get(col) ?? (original === null ? { kind: "null" } : { kind: "value", text: original });
      out.push({ gridRow: r, col, current, original });
    }
  }
  return out;
}

/** `value` in every cell, each checked against its column; NULL and DEFAULT included. */
export function setValue(cells: readonly SelectedCell[], rules: Rules, value: CellInput): Outcome {
  const out = new Collector();
  for (const cell of cells) {
    const rule = rules(cell.col);
    out.put(cell, rule, rule ? normalize(rule, value) : value);
  }
  return out.outcome();
}

/** Each column's value in the first selected row goes into the cells below it. */
export function fillDown(cells: readonly SelectedCell[], rules: Rules): Outcome {
  const out = new Collector();
  const first = new Map<number, SelectedCell>();
  for (const cell of cells) {
    const top = first.get(cell.col);
    if (!top || cell.gridRow < top.gridRow) first.set(cell.col, cell);
  }
  for (const cell of cells) {
    const top = first.get(cell.col)!;
    if (cell !== top) out.put(cell, rules(cell.col), top.current);
  }
  return out.outcome();
}

export interface ReplaceOptions {
  caseSensitive: boolean;
  regex: boolean;
}

/** Replaces the matches in one text, with how many there were. */
export type Replacer = (text: string) => { text: string; count: number };

/**
 * The replacement of `find` by `replacement`: as typed, or a regular expression whose
 * replacement may refer to groups (`$1`). `null` when there is nothing to find; the error's
 * message for an invalid expression.
 */
export function replacer(find: string, replacement: string, options: ReplaceOptions): Replacer | { error: string } | null {
  if (find === "") return null;
  const source = options.regex ? find : find.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  let re: RegExp;
  try {
    re = new RegExp(source, options.caseSensitive ? "g" : "gi");
  } catch (e) {
    return { error: (e as Error).message };
  }
  return (text) => {
    const count = text.match(re)?.length ?? 0;
    if (count === 0) return { text, count };
    // Typed text is put back as it is: `$` means nothing in it.
    return { text: options.regex ? text.replace(re, replacement) : text.replace(re, () => replacement), count };
  };
}

export interface Replaced extends Outcome {
  /** Matches replaced, over all cells. */
  replacements: number;
}

/** `replace` in the text cells of the selection; other columns, NULL and DEFAULT are left. */
export function replaceText(cells: readonly SelectedCell[], rules: Rules, replace: Replacer): Replaced {
  const out = new Collector();
  let replacements = 0;
  for (const cell of cells) {
    const rule = rules(cell.col);
    if (rule?.kind !== "text" || cell.current.kind !== "value") continue;
    const { text, count } = replace(cell.current.text);
    if (count === 0) continue;
    replacements += count;
    out.put(cell, rule, { kind: "value", text });
  }
  return { ...out.outcome(), replacements };
}

const decimals = (n: string) => {
  const [, frac = ""] = n.replace(/[eE].*$/, "").split(".");
  return frac.length;
};

/**
 * The `i`-th number of the sequence from `start` by `step`: exact for whole numbers of any
 * size, else with as many decimals as the two numbers have. `null` when one is not a number.
 */
export function sequenceValue(start: string, step: string, i: number): string | null {
  const a = start.trim();
  const b = step.trim();
  if (INTEGER.test(a) && INTEGER.test(b)) return (BigInt(a) + BigInt(b) * BigInt(i)).toString();
  if (!NUMBER.test(a) || !NUMBER.test(b)) return null;
  const value = Number(a) + Number(b) * i;
  return /e/i.test(a + b) ? String(value) : value.toFixed(Math.max(decimals(a), decimals(b)));
}

/** Numbers from `start` by `step` down each selected number column; `null` when `start` or
 *  `step` is not a number. Other columns are reported. */
export function numberSequence(cells: readonly SelectedCell[], rules: Rules, start: string, step: string): Outcome | null {
  if (sequenceValue(start, step, 0) === null) return null;
  const out = new Collector();
  const index = new Map<number, number>();
  // Down each column: by row, whatever order the cells come in.
  const ordered = [...cells].sort((x, y) => x.gridRow - y.gridRow);
  for (const cell of ordered) {
    const rule = rules(cell.col);
    if (rule && !isNumberRule(rule)) {
      out.reject(cell.col, "not_number");
      continue;
    }
    const i = index.get(cell.col) ?? 0;
    index.set(cell.col, i + 1);
    out.put(cell, rule, { kind: "value", text: sequenceValue(start, step, i)! });
  }
  return out.outcome();
}

/** Cells changed and left as they were, for a summary. */
export function counts(outcome: Outcome): { changed: number; rejected: number } {
  return { changed: outcome.changes.length, rejected: outcome.rejected.reduce((n, r) => n + r.cells, 0) };
}
