/**
 * Computed columns: an expression on the columns of the same row, worked out by the grid for
 * the rows it shows. They are not part of the result, so the features that read the result on
 * the backend (copy as, totals, search, edits, sorting) leave them out.
 *
 * The expressions are a small SQL-like language, evaluated without `eval`:
 * numbers, 'text', NULL, TRUE and FALSE; column names, bare or "quoted"; `+ - * / %`, `||`
 * to join text, `= <> != < <= > >=`, AND, OR, NOT and parentheses; and the functions in
 * `FUNCTIONS`. As in SQL, NULL in an operation gives NULL; a value that is not a number in an
 * arithmetic operation gives NULL too.
 */
import type { ColumnInfo } from "../api";

export interface ComputedColumn {
  name: string;
  expression: string;
}

/** A value while evaluating: text, a number, a truth value or NULL. */
type Value = string | number | boolean | null;

/** Works out a row's value from its cells' text; `undefined` while a cell is not loaded. */
type Evaluate = ((cell: (col: number) => string | null | undefined) => string | null | undefined) & {
  /** The result columns the expression reads. */
  uses: number[];
};

/** Why an expression cannot be used, with the text it is about. */
export class ExpressionError extends Error {
  readonly reason: "syntax" | "column" | "function" | "arguments";
  readonly near: string;

  constructor(reason: ExpressionError["reason"], near: string) {
    super(`${reason}: ${near}`);
    this.reason = reason;
    this.near = near;
  }
}

type Token =
  | { kind: "number"; value: number; text: string }
  | { kind: "string"; value: string; text: string }
  | { kind: "name"; value: string; quoted: boolean; text: string }
  | { kind: "op"; value: string; text: string }
  | { kind: "end"; text: string };

/** A number literal, shared by the tokenizer and by the reading of a cell's text as a number. */
const NUMBER = String.raw`(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?`;
const NUMBER_TOKEN = new RegExp(`^${NUMBER}`);
const NUMBER_TEXT = new RegExp(`^[+-]?${NUMBER}$`);

const OPERATORS = ["<>", "!=", "<=", ">=", "||", "+", "-", "*", "/", "%", "=", "<", ">", "(", ")", ","];

function tokenize(source: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  while (i < source.length) {
    const c = source[i]!;
    if (/\s/.test(c)) {
      i++;
      continue;
    }
    const rest = source.slice(i);
    const number = NUMBER_TOKEN.exec(rest);
    if (number) {
      tokens.push({ kind: "number", value: Number(number[0]), text: number[0] });
      i += number[0].length;
      continue;
    }
    if (c === "'" || c === '"' || c === "[" || c === "`") {
      const close = c === "[" ? "]" : c;
      let text = "";
      let j = i + 1;
      for (;;) {
        if (j >= source.length) throw new ExpressionError("syntax", rest);
        if (source[j] === close) {
          // A doubled quote stands for itself.
          if (close !== "]" && source[j + 1] === close) {
            text += close;
            j += 2;
            continue;
          }
          break;
        }
        text += source[j];
        j++;
      }
      const raw = source.slice(i, j + 1);
      tokens.push(c === "'" ? { kind: "string", value: text, text: raw } : { kind: "name", value: text, quoted: true, text: raw });
      i = j + 1;
      continue;
    }
    const word = /^[\p{L}_][\p{L}\p{N}_$]*/u.exec(rest);
    if (word) {
      tokens.push({ kind: "name", value: word[0], quoted: false, text: word[0] });
      i += word[0].length;
      continue;
    }
    const op = OPERATORS.find((o) => rest.startsWith(o));
    if (!op) throw new ExpressionError("syntax", rest);
    tokens.push({ kind: "op", value: op, text: op });
    i += op.length;
  }
  tokens.push({ kind: "end", text: "" });
  return tokens;
}

type Node = (cell: (col: number) => Value | undefined) => Value | undefined;

/** A number from a value: numbers as they are, text when it reads as one, else `null`. */
function toNumber(v: Value): number | null {
  if (typeof v === "number") return v;
  if (typeof v === "boolean") return v ? 1 : 0;
  if (v === null) return null;
  const text = v.trim();
  return NUMBER_TEXT.test(text) ? Number(text) : null;
}

function toText(v: Value): string | null {
  if (v === null) return null;
  if (typeof v === "boolean") return v ? "true" : "false";
  if (typeof v === "number") return formatNumber(v);
  return v;
}

/** A number without the noise of binary fractions (0.1 + 0.2 shows 0.3). */
function formatNumber(n: number): string {
  if (!Number.isFinite(n)) return String(n);
  return Number.isInteger(n) ? String(n) : String(Number(n.toPrecision(15)));
}

function toBool(v: Value): boolean | null {
  if (v === null) return null;
  if (typeof v === "boolean") return v;
  if (typeof v === "number") return v !== 0;
  const t = v.trim().toLowerCase();
  if (t === "true" || t === "t" || t === "yes") return true;
  if (t === "false" || t === "f" || t === "no") return false;
  const n = toNumber(v);
  return n === null ? null : n !== 0;
}

/** Compares two values: as numbers when both read as numbers, else as text. */
function compare(a: Value, b: Value): number | null {
  if (a === null || b === null) return null;
  const x = toNumber(a);
  const y = toNumber(b);
  if (x !== null && y !== null) return x === y ? 0 : x < y ? -1 : 1;
  const s = toText(a)!;
  const t = toText(b)!;
  return s === t ? 0 : s < t ? -1 : 1;
}

/** Calls `f` with the arguments' values, `undefined` while one is not loaded. */
const all =
  (args: Node[], f: (values: Value[]) => Value): Node =>
  (cell) => {
    const values: Value[] = [];
    for (const a of args) {
      const v = a(cell);
      if (v === undefined) return undefined;
      values.push(v);
    }
    return f(values);
  };

/** As `all`, NULL when any argument is NULL. */
const strict = (args: Node[], f: (values: Value[]) => Value): Node =>
  all(args, (values) => (values.includes(null) ? null : f(values)));

const numeric = (f: (...n: number[]) => Value) => (values: Value[]) => {
  const numbers = values.map(toNumber);
  return numbers.includes(null) ? null : f(...(numbers as number[]));
};

interface FunctionDef {
  min: number;
  max: number;
  build: (args: Node[]) => Node;
}

const fn = (min: number, max: number, build: (args: Node[]) => Node): FunctionDef => ({ min, max, build });

/** Division by zero, and any other operation without a number, gives NULL, as in most
 *  databases' lenient modes. */
const orNull = (n: number) => (Number.isNaN(n) ? null : n);

type Arithmetic = Record<string, (a: number, b: number) => Value>;

const ADDITIVE: Arithmetic = {
  "+": (a, b) => a + b,
  "-": (a, b) => a - b,
};

const MULTIPLICATIVE: Arithmetic = {
  "*": (a, b) => orNull(a * b),
  "/": (a, b) => (b === 0 ? null : orNull(a / b)),
  "%": (a, b) => (b === 0 ? null : orNull(a % b)),
};

/** The comparison operators, from the result of `compare`. */
const COMPARISONS: Record<string, (c: number) => boolean> = {
  "=": (c) => c === 0,
  "<>": (c) => c !== 0,
  "!=": (c) => c !== 0,
  "<": (c) => c < 0,
  "<=": (c) => c <= 0,
  ">": (c) => c > 0,
  ">=": (c) => c >= 0,
};

/** The functions, by lower-case name. */
const FUNCTIONS: Record<string, FunctionDef> = {
  upper: fn(1, 1, (a) => strict(a, ([v]) => toText(v!)!.toUpperCase())),
  lower: fn(1, 1, (a) => strict(a, ([v]) => toText(v!)!.toLowerCase())),
  trim: fn(1, 1, (a) => strict(a, ([v]) => toText(v!)!.trim())),
  length: fn(1, 1, (a) => strict(a, ([v]) => [...toText(v!)!].length)),
  substr: fn(2, 3, (a) =>
    strict(a, ([v, start, len]) => {
      const chars = [...toText(v!)!];
      const from = toNumber(start!);
      const count = len === undefined ? chars.length : toNumber(len);
      if (from === null || count === null) return null;
      // 1-based, as in SQL.
      return chars.slice(Math.max(0, from - 1), Math.max(0, from - 1 + count)).join("");
    }),
  ),
  replace: fn(3, 3, (a) => strict(a, ([v, from, to]) => toText(v!)!.split(toText(from!)!).join(toText(to!)!))),
  abs: fn(1, 1, (a) => strict(a, numeric(Math.abs))),
  round: fn(1, 2, (a) =>
    strict(
      a,
      numeric((n, digits = 0) => {
        const f = 10 ** Math.trunc(digits);
        return Math.round(n * f) / f;
      }),
    ),
  ),
  floor: fn(1, 1, (a) => strict(a, numeric(Math.floor))),
  ceil: fn(1, 1, (a) => strict(a, numeric(Math.ceil))),
  coalesce: fn(1, Infinity, (args) => (cell) => {
    for (const a of args) {
      const v = a(cell);
      if (v === undefined) return undefined;
      if (v !== null) return v;
    }
    return null;
  }),
  nullif: fn(2, 2, (a) => all(a, ([x, y]) => (compare(x!, y!) === 0 ? null : x!))),
  concat: fn(1, Infinity, (a) => all(a, (values) => values.map((v) => toText(v) ?? "").join(""))),
  if: fn(3, 3, ([cond, yes, no]) => (cell) => {
    const c = cond!(cell);
    if (c === undefined) return undefined;
    return toBool(c) ? yes!(cell) : no!(cell);
  }),
};

/** The names of the functions, for the user. */
export const FUNCTION_NAMES = Object.keys(FUNCTIONS);

/** Recursive descent over the tokens, from the loosest operator to the tightest. */
class Parser {
  private at = 0;
  private readonly tokens: Token[];
  private readonly column: (name: string, quoted: boolean) => number | undefined;

  constructor(tokens: Token[], column: (name: string, quoted: boolean) => number | undefined) {
    this.tokens = tokens;
    this.column = column;
  }

  private peek(): Token {
    return this.tokens[this.at]!;
  }

  private next(): Token {
    return this.tokens[this.at++]!;
  }

  private isOp(value: string): boolean {
    const t = this.peek();
    return t.kind === "op" && t.value === value;
  }

  private isWord(word: string): boolean {
    const t = this.peek();
    return t.kind === "name" && !t.quoted && t.value.toLowerCase() === word;
  }

  private expect(value: string) {
    if (!this.isOp(value)) throw new ExpressionError("syntax", this.rest());
    this.at++;
  }

  private rest(): string {
    return this.tokens
      .slice(this.at)
      .map((t) => t.text)
      .join(" ")
      .trim();
  }

  parse(): Node {
    const node = this.or();
    if (this.peek().kind !== "end") throw new ExpressionError("syntax", this.rest());
    return node;
  }

  private or(): Node {
    return this.logical("or", true, () => this.and());
  }

  private and(): Node {
    return this.logical("and", false, () => this.not());
  }

  /** OR and AND: `decisive` on either side decides; else NULL on either side gives NULL. */
  private logical(word: string, decisive: boolean, operand: () => Node): Node {
    let left = operand();
    while (this.isWord(word)) {
      this.at++;
      const l = left;
      const r = operand();
      left = (cell) => {
        const a = l(cell);
        const b = r(cell);
        if (a === undefined || b === undefined) return undefined;
        const x = toBool(a);
        const y = toBool(b);
        return x === decisive || y === decisive ? decisive : x === null || y === null ? null : !decisive;
      };
    }
    return left;
  }

  private not(): Node {
    if (this.isWord("not")) {
      this.at++;
      const inner = this.not();
      return strict([inner], ([v]) => {
        const b = toBool(v!);
        return b === null ? null : !b;
      });
    }
    return this.comparison();
  }

  private comparison(): Node {
    const left = this.concat();
    const t = this.peek();
    const ok = t.kind === "op" ? COMPARISONS[t.value] : undefined;
    if (ok) {
      this.at++;
      const right = this.concat();
      return all([left, right], ([a, b]) => {
        const c = compare(a!, b!);
        return c === null ? null : ok(c);
      });
    }
    if (this.isWord("is")) {
      this.at++;
      const negate = this.isWord("not");
      if (negate) this.at++;
      if (!this.isWord("null")) throw new ExpressionError("syntax", this.rest());
      this.at++;
      return all([left], ([v]) => (v === null) !== negate);
    }
    return left;
  }

  private concat(): Node {
    let left = this.additive();
    while (this.isOp("||")) {
      this.at++;
      const right = this.additive();
      left = strict([left, right], ([a, b]) => toText(a!)! + toText(b!)!);
    }
    return left;
  }

  private additive(): Node {
    return this.arithmetic(ADDITIVE, () => this.multiplicative());
  }

  private multiplicative(): Node {
    return this.arithmetic(MULTIPLICATIVE, () => this.unary());
  }

  /** A left-associative chain of the operators in `ops`. */
  private arithmetic(ops: Arithmetic, operand: () => Node): Node {
    let left = operand();
    for (;;) {
      const t = this.peek();
      const op = t.kind === "op" ? ops[t.value] : undefined;
      if (!op) return left;
      this.at++;
      left = strict([left, operand()], numeric(op));
    }
  }

  private unary(): Node {
    if (this.isOp("-")) {
      this.at++;
      return strict([this.unary()], numeric((a) => -a));
    }
    if (this.isOp("+")) {
      this.at++;
      return this.unary();
    }
    return this.primary();
  }

  private primary(): Node {
    const t = this.next();
    if (t.kind === "number") return () => t.value;
    if (t.kind === "string") return () => t.value;
    if (t.kind === "op" && t.value === "(") {
      const inner = this.or();
      this.expect(")");
      return inner;
    }
    if (t.kind === "name") {
      if (!t.quoted && this.isOp("(")) return this.call(t.value);
      if (!t.quoted) {
        const word = t.value.toLowerCase();
        if (word === "null") return () => null;
        if (word === "true") return () => true;
        if (word === "false") return () => false;
      }
      const col = this.column(t.value, t.quoted);
      if (col === undefined) throw new ExpressionError("column", t.value);
      return (cell) => cell(col);
    }
    throw new ExpressionError("syntax", t.text || this.rest());
  }

  private call(name: string): Node {
    const def = FUNCTIONS[name.toLowerCase()];
    if (!def) throw new ExpressionError("function", name);
    this.expect("(");
    const args: Node[] = [];
    if (!this.isOp(")")) {
      args.push(this.or());
      while (this.isOp(",")) {
        this.at++;
        args.push(this.or());
      }
    }
    this.expect(")");
    if (args.length < def.min || args.length > def.max) throw new ExpressionError("arguments", name);
    return def.build(args);
  }
}

/**
 * Compiles `expression` over `columns`: names match exactly, then ignoring case when only one
 * column has that name. Throws an `ExpressionError` when it cannot be used.
 */
export function compile(expression: string, columns: ColumnInfo[]): Evaluate {
  const column = (name: string, quoted: boolean) => {
    const exact = columns.findIndex((c) => c.name === name);
    if (exact >= 0) return exact;
    if (quoted) return undefined;
    const folded = columns.flatMap((c, i) => (c.name.toLowerCase() === name.toLowerCase() ? [i] : []));
    return folded.length === 1 ? folded[0] : undefined;
  };
  const uses = new Set<number>();
  const node = new Parser(tokenize(expression), (name, quoted) => {
    const col = column(name, quoted);
    if (col !== undefined) uses.add(col);
    return col;
  }).parse();
  const evaluate = (cell: (col: number) => string | null | undefined) => {
    const v = node(cell);
    return v === undefined ? undefined : toText(v);
  };
  return Object.assign(evaluate, { uses: [...uses].sort((a, b) => a - b) });
}

/** The computed columns that compile over `columns`, with the error of the others. */
export function compileAll(computed: ComputedColumn[], columns: ColumnInfo[]) {
  return computed.map((c) => {
    try {
      return { column: c, evaluate: compile(c.expression, columns), error: null };
    } catch (e) {
      return { column: c, evaluate: null, error: e instanceof ExpressionError ? e : new ExpressionError("syntax", c.expression) };
    }
  });
}

/** The header of a computed column: its name, and its expression in place of the type. */
export function computedInfo(c: ComputedColumn): ColumnInfo {
  const shown = c.expression.length > 28 ? `${c.expression.slice(0, 27)}…` : c.expression;
  return { name: c.name, db_type: `= ${shown}`, numeric: false, binary: false };
}

/** Computed columns of data viewer tables, kept in this browser profile. */
const KEY = "platypus.computed.v1";

function stored(): Record<string, ComputedColumn[]> {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) ?? "{}") as unknown;
    return parsed && typeof parsed === "object" ? (parsed as Record<string, ComputedColumn[]>) : {};
  } catch {
    return {};
  }
}

export function loadComputed(key: string): ComputedColumn[] {
  const list = stored()[key];
  return Array.isArray(list) ? list.filter((c) => typeof c?.name === "string" && typeof c?.expression === "string") : [];
}

export function saveComputed(key: string, list: ComputedColumn[]) {
  try {
    const all = stored();
    if (list.length === 0) delete all[key];
    else all[key] = list;
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    // Storage can be unavailable or full; computed columns are a convenience.
  }
}
