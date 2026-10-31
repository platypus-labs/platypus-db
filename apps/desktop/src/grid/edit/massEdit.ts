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
  /** DEFAULT gives the column a value: it has a default, takes NULL, or is a key the
   *  database numbers (identity, auto-increment, SQLite's rowid). */
  takesDefault: boolean;
  /** An empty value is NULL: the column holds no text that could be empty (numbers, dates
   *  and times, booleans, JSON, UUIDs, network addresses). */
  emptyIsNull: boolean;
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
  /** DEFAULT in a NOT NULL column without a default. */
  | "default"
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

// `JSON_TYPE` of jsonText.ts: this module imports no values, so that its tests run in Node.
const JSON_TYPE = /json/i;
const BOOL_TYPE = /^bool(ean)?$/i;
const INTEGER_TYPE = /\b(tiny|small|medium|big)?int(eger)?\d*\b|\b(small|big)?serial\d*\b|\bu?int(8|16|32|64|128)\b/i;
const NUMBER_TYPE = /\b(numeric|decimal|dec|real|double|float\d*|number|money|smallmoney)\b/i;
const FLOAT_TYPE = /\b(real|double|float\d*)\b/i;
const TEXT_TYPE = /char|text|clob|string|xml|\bname\b/i;
/** Other types without an empty value: dates and times, UUIDs, network addresses. */
const VALUE_TYPE = /^(nullable\()?(date|time|timestamp|datetime|smalldatetime|interval|year|uuid|uniqueidentifier|inet|cidr|macaddr)/i;
const INTEGER = /^[+-]?\d+$/;
const NUMBER = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;
const FLOAT_WORDS = new Set(["nan", "infinity", "+infinity", "-infinity", "inf", "-inf"]);
const BOOL_WORDS = new Set(["true", "t", "1", "yes", "y", "on", "false", "f", "0", "no", "n", "off"]);

/** The rule of a result column, from its type and the table's catalog. */
export function columnRule(column: ColumnInfo, meta: ColumnMeta | undefined): ColumnRule {
  const type = meta?.data_type ?? column.db_type;
  const rule = (kind: ValueKind): ColumnRule => ({
    kind,
    nullable: meta?.nullable ?? true,
    enumValues: meta?.enum_values ?? [],
    float: FLOAT_TYPE.test(type),
    // The catalog does not say which columns the database numbers: those are keys.
    takesDefault: !meta || meta.nullable || meta.default !== null || meta.primary_key,
    emptyIsNull:
      kind === "integer" || kind === "number" || kind === "bool" || kind === "json" || (kind === "other" && VALUE_TYPE.test(type)),
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

/** An empty value is NULL where the column holds no empty text, as in the cell editor. */
export function normalize(rule: ColumnRule, input: CellInput): CellInput {
  return rule.emptyIsNull && input.kind === "value" && input.text.trim() === "" ? { kind: "null" } : input;
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

const isHexDigit = (c: number) => (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x46) || (c >= 0x61 && c <= 0x66);

/** Whole bytes in hexadecimal, after an optional `\x` or `0x`. A loop: a regular expression
 *  over the hex of a file of megabytes overflows the stack. */
export function isHex(text: string): boolean {
  const start = text.startsWith("\\x") || text.startsWith("0x") ? 2 : 0;
  if ((text.length - start) % 2 !== 0) return false;
  for (let i = start; i < text.length; i++) if (!isHexDigit(text.charCodeAt(i))) return false;
  return true;
}

/** Why `input` does not fit a column with `rule`; `null` when it does. */
export function checkValue(rule: ColumnRule, input: CellInput): Problem | null {
  if (input.kind === "default") return rule.takesDefault ? null : "default";
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
      return isHex(text) ? null : "binary";
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
 * the full values of the result rows from `firstLoaded` on, of the columns `cols` only and in
 * their order (`api.rowValues` with `cols`).
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
    selection.cols.forEach((col, i) => {
      const original = values[i] ?? null;
      const current: CellInput = pending?.get(col) ?? (original === null ? { kind: "null" } : { kind: "value", text: original });
      out.push({ gridRow: r, col, current, original });
    });
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

/** One text after a replacement, with how many matches were replaced in it. */
export interface TextReplaced {
  text: string;
  count: number;
}

/** Replaces the matches in one text. */
export type Replacer = (text: string) => TextReplaced;

/**
 * The replacement of `find` by `replacement`: as typed, or a regular expression whose
 * replacement may refer to groups (`$1`). `null` when there is nothing to find; the error's
 * message for an invalid expression. Expressions match whole characters (the `u` flag), so an
 * emoji is never split in two; one the `u` flag refuses but the usual syntax takes (`\-`,
 * `[\w-.]`) is used without it.
 */
export function replacer(find: string, replacement: string, options: ReplaceOptions): Replacer | { error: string } | null {
  if (find === "") return null;
  const source = options.regex ? find : find.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const flags = options.caseSensitive ? "g" : "gi";
  let re: RegExp;
  try {
    re = new RegExp(source, `${flags}u`);
  } catch {
    try {
      re = new RegExp(source, flags);
    } catch (e) {
      return { error: (e as Error).message };
    }
  }
  return (text) => {
    const count = text.match(re)?.length ?? 0;
    if (count === 0) return { text, count };
    // Typed text is put back as it is: `$` means nothing in it.
    const out = options.regex ? text.replace(re, replacement) : text.replace(re, () => replacement);
    // Matches that change nothing, e.g. empty ones replaced by nothing, are no replacements.
    return out === text ? { text, count: 0 } : { text: out, count };
  };
}

export interface Replaced extends Outcome {
  /** Matches replaced, over all cells. */
  replacements: number;
}

/** What a replacement searches: the text cells holding a value (by index in `cells`), and
 *  their texts. NULL, DEFAULT and other columns are left. */
export function replaceInputs(cells: readonly SelectedCell[], rules: Rules): { targets: number[]; texts: string[] } {
  const targets: number[] = [];
  const texts: string[] = [];
  cells.forEach((cell, i) => {
    if (rules(cell.col)?.kind !== "text" || cell.current.kind !== "value") return;
    targets.push(i);
    texts.push(cell.current.text);
  });
  return { targets, texts };
}

/** The outcome of a replacement from its `results`, one per target of `replaceInputs`
 *  (`null` where nothing matched). */
export function replacedOutcome(
  cells: readonly SelectedCell[],
  rules: Rules,
  targets: readonly number[],
  results: readonly (TextReplaced | null)[],
): Replaced {
  const out = new Collector();
  let replacements = 0;
  targets.forEach((index, k) => {
    const result = results[k];
    const cell = cells[index];
    if (!result || !cell) return;
    replacements += result.count;
    out.put(cell, rules(cell.col), { kind: "value", text: result.text });
  });
  return { ...out.outcome(), replacements };
}

const replaceOne = (replace: Replacer, text: string): TextReplaced | null => {
  const result = replace(text);
  return result.count > 0 ? result : null;
};

/** `replace` in the text cells of the selection; other columns, NULL and DEFAULT are left. */
export function replaceText(cells: readonly SelectedCell[], rules: Rules, replace: Replacer): Replaced {
  const { targets, texts } = replaceInputs(cells, rules);
  return replacedOutcome(
    cells,
    rules,
    targets,
    texts.map((text) => replaceOne(replace, text)),
  );
}

/** A replacement run away from the UI thread (`replaceWorker.ts`). */
export interface ReplaceJob extends ReplaceOptions {
  texts: string[];
  find: string;
  replacement: string;
}

/** What the run of a `ReplaceJob` posts: how many texts are done, then the results (`null`
 *  where nothing matched), or why it could not run. */
export type ReplaceReply = { done: number } | { results: (TextReplaced | null)[] } | { error: string };

/** Runs `job`, posting progress at least every `every` ms while it works, so the UI can tell
 *  a slow expression from a stuck one. */
export function runReplaceJob(job: ReplaceJob, post: (reply: ReplaceReply) => void, now = () => performance.now(), every = 100) {
  const replace = replacer(job.find, job.replacement, job);
  if (typeof replace !== "function") return post({ error: replace?.error ?? "nothing to find" });
  const results: (TextReplaced | null)[] = [];
  let last = now();
  job.texts.forEach((text, i) => {
    results.push(replaceOne(replace, text));
    if (now() - last >= every) {
      last = now();
      post({ done: i + 1 });
    }
  });
  post({ results });
}

const decimals = (n: string) => {
  const [, frac = ""] = n.replace(/[eE].*$/, "").split(".");
  return frac.length;
};

const PLAIN_NUMBER = /^([+-]?)(\d*)(?:\.(\d*))?$/;

/** `text`, a number without exponent, times 10^`scale` (at least its decimals). */
function scaled(text: string, scale: number): bigint {
  const [, sign = "", whole = "", frac = ""] = PLAIN_NUMBER.exec(text) ?? [];
  return BigInt(`${sign === "-" ? "-" : ""}${whole || "0"}${frac.padEnd(scale, "0")}`);
}

/** `value` divided by 10^`scale`, written with `scale` decimals. */
function unscaled(value: bigint, scale: number): string {
  if (scale === 0) return value.toString();
  const negative = value < 0n;
  const digits = (negative ? -value : value).toString().padStart(scale + 1, "0");
  return `${negative ? "-" : ""}${digits.slice(0, -scale)}.${digits.slice(-scale)}`;
}

/**
 * The `i`-th number of the sequence from `start` by `step`, exact with as many decimals as the
 * two numbers have, whatever their size. With an exponent, a floating-point number. `null` when
 * one is not a number, or the value is beyond the floating-point range.
 */
export function sequenceValue(start: string, step: string, i: number): string | null {
  const a = start.trim();
  const b = step.trim();
  if (!NUMBER.test(a) || !NUMBER.test(b)) return null;
  if (/e/i.test(a + b)) {
    const value = Number(a) + Number(b) * i;
    return Number.isFinite(value) ? String(value) : null;
  }
  const scale = Math.max(decimals(a), decimals(b));
  return unscaled(scaled(a, scale) + scaled(b, scale) * BigInt(i), scale);
}

const WHOLE = /^[+-]?\d+(\.0*)?$/;

/** Numbers from `start` by `step` down each selected number column; `null` when `start` or
 *  `step` is not a number. Other columns are reported. */
export function numberSequence(cells: readonly SelectedCell[], rules: Rules, start: string, step: string): Outcome | null {
  if (sequenceValue(start, step, 0) === null) return null;
  // From `1.0` by `1` the numbers are whole: written as such in whole-number columns.
  const whole = WHOLE.test(start.trim()) && WHOLE.test(step.trim());
  const wholeIn = (rule: ColumnRule | undefined, text: string) =>
    whole && rule?.kind === "integer" ? text.replace(/\.\d*$/, "") : text;
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
    const text = sequenceValue(start, step, i);
    if (text === null) out.reject(cell.col, "number");
    else out.put(cell, rule, { kind: "value", text: wholeIn(rule, text) });
  }
  return out.outcome();
}

/** Cells changed and left as they were, for a summary. */
export function counts(outcome: Outcome): { changed: number; rejected: number } {
  return { changed: outcome.changes.length, rejected: outcome.rejected.reduce((n, r) => n + r.cells, 0) };
}
