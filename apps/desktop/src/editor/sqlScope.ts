// Names to complete at the cursor of one SQL statement: the columns of the tables it reads
// (before or after the cursor), the tables of a schema, the columns of `alias.`, of a CTE or
// of a subquery in FROM. Plain logic on the statement's text, without CodeMirror, so that it
// can be tested on its own (tests/sqlScope.test.mjs).
import type { CompletionCatalog, DriverKind } from "../api";

type Catalog = Pick<CompletionCatalog, "default_schema" | "schemas">;

interface Token {
  /** `word` is a bare name or keyword, `quoted` a quoted name, `punct` one character. */
  kind: "word" | "quoted" | "punct";
  text: string;
  from: number;
  to: number;
  /** Index of the `(` this token is inside, or -1 at the top level. */
  parent: number;
}

/** What the statement reads, under the name its columns are qualified with. */
export interface Source {
  /** The alias, or the table's name as written. */
  name: string;
  /** The same name with its quotes as written, to qualify a column with. */
  written?: string;
  /** The catalog table, or null for a CTE or a subquery. */
  schema: string | null;
  table: string | null;
  columns: string[];
}

export interface NameItem {
  label: string;
  kind: "column" | "table" | "schema" | "source";
  /** For a catalog table or column: where it is, to find its notes. */
  schema?: string;
  table?: string;
  /** For a column without qualifier: the source it belongs to. */
  source?: string;
  /** For such a column whose name other sources in scope have too: the source as written,
   *  to insert before it (`o.id`). */
  qualifier?: string;
}

export interface NameOptions {
  /** Start of the typed name (its opening quote included). */
  from: number;
  /** The opening quote the user typed, or null. */
  quote: string | null;
  /** Columns without qualifier, ranked above everything else. */
  unqualified: boolean;
  items: NameItem[];
}

export interface ScopeOptions {
  /** `[name]` quotes names (Transact-SQL). */
  brackets?: boolean;
  /** A backslash escapes the next character in strings and quoted names. */
  backslashes?: boolean;
  /** A backslash escapes the next character in `'…'` strings only. */
  stringBackslashes?: boolean;
  /** `"…"` is a string, not a name. */
  doubleQuotedStrings?: boolean;
  /** `E'…'` is a string where a backslash escapes the next character (PostgreSQL, DuckDB). */
  escapeStrings?: boolean;
  /** `$tag$…$tag$` is a string (PostgreSQL, DuckDB, ClickHouse's heredocs). */
  dollarQuotes?: boolean;
  /** `#` opens a line comment: anywhere (MySQL, BigQuery), or before a space or `!` (ClickHouse). */
  hashComments?: "always" | "spaced";
  /** `--` opens a comment only before a space or the end (MySQL). */
  spacedDashes?: boolean;
  /** `q'[…]'` is a string closed by its own delimiter (Oracle). */
  qQuotes?: boolean;
}

/** How the driver's SQL quotes and comments, as `lexer.rs` and `dialect.rs` in platypus-sql
 *  read it. */
export function scopeOptions(driver: DriverKind): ScopeOptions {
  const backslashes = ["mysql", "clickhouse", "bigquery", "databricks"].includes(driver);
  return {
    brackets: driver === "sqlserver",
    backslashes,
    stringBackslashes: backslashes || driver === "snowflake",
    doubleQuotedStrings: ["mysql", "bigquery", "databricks"].includes(driver),
    escapeStrings: driver === "postgres" || driver === "duckdb",
    dollarQuotes: ["postgres", "duckdb", "clickhouse"].includes(driver),
    hashComments: driver === "mysql" || driver === "bigquery" ? "always" : driver === "clickhouse" ? "spaced" : undefined,
    spacedDashes: driver === "mysql",
    qQuotes: driver === "oracle",
  };
}

/** Where the quoted text opened at `i` ends (after `close`), or -1 when it does not. `close`
 *  doubled, or after a backslash with `backslash`, is part of the text. */
function quotedEnd(sql: string, i: number, close: string, backslash: boolean): number {
  for (let end = i + 1; end < sql.length; end++) {
    if (backslash && sql[end] === "\\") end++;
    else if (sql[end] === close) {
      if (sql[end + 1] !== close) return end + 1;
      end++;
    }
  }
  return -1;
}

/** The quote that closes a name or string at `c`, and whether it is a name (kept by
 *  masking); null when `c` opens neither. */
function quoteAt(c: string, options: ScopeOptions): { close: string; name: boolean } | null {
  if (c === "'") return { close: "'", name: false };
  if (c === '"') return { close: '"', name: !options.doubleQuotedStrings };
  if (c === "`") return { close: "`", name: true };
  if (c === "[" && options.brackets) return { close: "]", name: true };
  return null;
}

const backslashIn = (q: { name: boolean; close: string }, options: ScopeOptions) =>
  !!options.backslashes || (!q.name && !!options.stringBackslashes);

/** A character of a bare name. */
const NAME_CHAR = /[\w$#\u0080-￿]/;

/** Whether an `E'…'` string starts at `i`: an `E` of its own, not the end of a name. */
const escapeStringAt = (sql: string, i: number, options: ScopeOptions) =>
  !!options.escapeStrings && (sql[i] === "E" || sql[i] === "e") && sql[i + 1] === "'" && !NAME_CHAR.test(sql[i - 1] ?? "");

/** Where the comment that starts at `i` ends (its newline), or -1 when none starts there. */
function lineCommentEnd(sql: string, i: number, options: ScopeOptions): number {
  const next = sql[i + 1];
  const spaced = next === undefined || /\s/.test(next);
  const dashes = sql[i] === "-" && next === "-" && (!options.spacedDashes || sql[i + 2] === undefined || /\s/.test(sql[i + 2]!));
  const hash =
    sql[i] === "#" && (options.hashComments === "always" || (options.hashComments === "spaced" && (spaced || next === "!")));
  if (!dashes && !hash) return -1;
  const end = sql.indexOf("\n", i);
  return end < 0 ? sql.length : end;
}

/** Where the string that a `$tag$` or an Oracle `q'[` opens at `i` ends, or -1 when none starts
 *  there: a `$` or `q` of its own, not part of a name. */
function taggedStringEnd(sql: string, i: number, options: ScopeOptions): number {
  if (NAME_CHAR.test(sql[i - 1] ?? "")) return -1;
  if (options.dollarQuotes && sql[i] === "$") {
    const tag = /^\$(?:[A-Za-z_]\w*)?\$/.exec(sql.slice(i, i + 64))?.[0];
    if (!tag) return -1;
    const end = sql.indexOf(tag, i + tag.length);
    return end < 0 ? sql.length : end + tag.length;
  }
  const q = options.qQuotes ? /^[nN]?[qQ]'(.)/su.exec(sql.slice(i, i + 5)) : null;
  if (!q) return -1;
  const close = ({ "[": "]", "{": "}", "<": ">", "(": ")" } as Record<string, string>)[q[1]!] ?? q[1]!;
  const end = sql.indexOf(`${close}'`, i + q[0].length);
  return end < 0 ? sql.length : end + close.length + 1;
}

const WORD = /[A-Za-z_\u0080-￿][\w$#\u0080-￿]*/y;
/** Words after which a table is named. */
const TABLE_KEYWORDS = new Set(["FROM", "JOIN", "UPDATE", "INTO", "TABLE"]);
/** What may follow a table without being its alias. */
const NOT_ALIAS = new Set(
  (
    "ON USING WHERE JOIN LEFT RIGHT INNER OUTER FULL CROSS NATURAL GROUP ORDER LIMIT OFFSET " +
    "HAVING UNION INTERSECT EXCEPT MINUS SET WINDOW LATERAL AS VALUES SELECT FOR RETURNING DEFAULT " +
    "WITH FETCH QUALIFY TABLESAMPLE STRAIGHT_JOIN USE FORCE IGNORE INDEXED NOT WHEN OR AND " +
    // ClickHouse, DuckDB, Oracle and MySQL words that may follow a table.
    "FINAL PREWHERE SAMPLE GLOBAL ARRAY ANY SEMI ANTI ASOF POSITIONAL START CONNECT PIVOT UNPIVOT PARTITION"
  ).split(" "),
);
/** Words that end the select list. */
const END_SELECT = new Set(
  "FROM INTO WHERE GROUP HAVING ORDER LIMIT OFFSET FETCH UNION INTERSECT EXCEPT MINUS WINDOW QUALIFY".split(" "),
);
/** Words that start a clause, to tell which one a list is in. */
const CLAUSES = new Set([...END_SELECT, "SELECT", "SET", "ON", "JOIN"]);
/** Words a `USING` may belong to: only DELETE and MERGE take tables there. */
const USING_OWNERS = new Set(["DELETE", "MERGE", "JOIN", "SELECT", "CREATE", "ALTER"]);
/** Words an `INTO` may belong to; the target of the first two is out of its rows' query. */
const INSERTS = new Set(["INSERT", "REPLACE"]);
const INTO_OWNERS = new Set([...INSERTS, "MERGE", "SELECT"]);

/** The SQL with strings and comments blanked out, the same length. */
export function maskSql(sql: string, options: ScopeOptions = {}): string {
  let out = "";
  let i = 0;
  const blank = (s: string) => s.replace(/[^\n]/g, " ");
  while (i < sql.length) {
    const c = sql[i]!;
    let end = -1;
    let keep = false;
    const quote = quoteAt(c, options);
    if (quote) {
      // A quoted name stays, but a quote inside it starts no string.
      const close = quotedEnd(sql, i, quote.close, backslashIn(quote, options));
      end = close < 0 ? sql.length : close;
      keep = quote.name;
    } else if (escapeStringAt(sql, i, options)) {
      const close = quotedEnd(sql, i + 1, "'", true);
      end = close < 0 ? sql.length : close;
    } else if (c === "/" && sql[i + 1] === "*") {
      end = sql.indexOf("*/", i + 2);
      end = end < 0 ? sql.length : end + 2;
    } else if (c === "-" || c === "#") {
      end = lineCommentEnd(sql, i, options);
    } else if ((c === "$" && options.dollarQuotes) || (options.qQuotes && "qQnN".includes(c))) {
      end = taggedStringEnd(sql, i, options);
    }
    if (end < 0) {
      out += c;
      i++;
    } else {
      end = Math.min(end, sql.length);
      out += keep ? sql.slice(i, end) : blank(sql.slice(i, end));
      i = end;
    }
  }
  return out;
}

/** The statement around `pos`: from after the `;` before it to the `;` after it. */
export function statementAt(doc: string, pos: number, options: ScopeOptions = {}): { from: number; to: number } {
  const text = maskSql(doc, options);
  const end = text.indexOf(";", pos);
  return { from: text.lastIndexOf(";", pos - 1) + 1, to: end < 0 ? text.length : end };
}

function tokenize(sql: string, options: ScopeOptions): Token[] {
  const text = maskSql(sql, options);
  const tokens: Token[] = [];
  const stack: number[] = [];
  let i = 0;
  while (i < text.length) {
    const c = text[i]!;
    if (/\s/.test(c)) {
      i++;
      continue;
    }
    const parent = stack.at(-1) ?? -1;
    // Strings are blanked by now: a quote left opens a name.
    const quote = quoteAt(c, options);
    if (quote) {
      const close = quotedEnd(text, i, quote.close, backslashIn(quote, options));
      const end = close < 0 ? text.length : close;
      tokens.push({ kind: "quoted", text: text.slice(i, end), from: i, to: end, parent });
      i = end;
      continue;
    }
    WORD.lastIndex = i;
    const word = WORD.exec(text);
    if (word) {
      tokens.push({ kind: "word", text: word[0], from: i, to: i + word[0].length, parent });
      i += word[0].length;
      continue;
    }
    if (c === ")") stack.pop();
    tokens.push({ kind: "punct", text: c, from: i, to: i + 1, parent: c === ")" ? (stack.at(-1) ?? -1) : parent });
    if (c === "(") stack.push(tokens.length - 1);
    i++;
  }
  return tokens;
}

/** The quote that closes a name opened by `open`. */
export const closingQuote = (open: string) => (open === "[" ? "]" : open);

/** A quoted name without its quotes. */
export function unquote(ident: string): string {
  const q = ident[0];
  if (q !== '"' && q !== "`" && q !== "[") return ident;
  const close = closingQuote(q);
  return ident.length >= 2 && ident.endsWith(close) ? ident.slice(1, -1).replaceAll(close + close, close) : ident;
}

const isName = (t: Token | undefined) => !!t && (t.kind === "quoted" || t.kind === "word");
/** A name that is not a keyword ending a table reference. */
const isAliasable = (t: Token | undefined) => isName(t) && !(t!.kind === "word" && NOT_ALIAS.has(t!.text.toUpperCase()));
const isWord = (t: Token | undefined, ...words: string[]) => t?.kind === "word" && words.includes(t.text.toUpperCase());
const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** The item of `items` named `name`: exactly, else in another case. */
function byName<T>(items: T[], name: string, nameOf: (item: T) => string): T | undefined {
  return items.find((x) => nameOf(x) === name) ?? items.find((x) => same(nameOf(x), name));
}

/** The key of `record` named `name`: exactly, else in another case. */
const keyNamed = (record: object, name: string) =>
  name in record ? name : Object.keys(record).find((k) => same(k, name));

/** Index of the `)` closing the `(` at `open`, or `tokens.length`. */
function closing(tokens: Token[], open: number) {
  for (let i = open + 1; i < tokens.length; i++) {
    if (tokens[i]!.text === ")" && tokens[i]!.parent === tokens[open]!.parent) return i;
  }
  return tokens.length;
}

function schemaNamed(catalog: Catalog, name: string): string | undefined {
  return keyNamed(catalog.schemas, name);
}

/** Looks up a table: in `schema`, or in the default schema and then in any other. */
function catalogTable(catalog: Catalog, schema: string | null, table: string): { schema: string; table: string; columns: string[] } | null {
  const find = (s: string) => {
    const tables = catalog.schemas[s];
    if (!tables) return null;
    const name = keyNamed(tables, table);
    return name === undefined ? null : { schema: s, table: name, columns: tables[name]! };
  };
  if (schema !== null) {
    const s = schemaNamed(catalog, schema);
    return s === undefined ? null : find(s);
  }
  const schemas = Object.keys(catalog.schemas);
  for (const s of catalog.default_schema ? [catalog.default_schema, ...schemas] : schemas) {
    const found = find(s);
    if (found) return found;
  }
  return null;
}

interface Cte {
  name: string;
  /** The column list written after the name, if any. */
  columns: string[] | null;
  open: number;
  close: number;
}

interface Ref {
  /** The source, without columns for a subquery until they are needed. */
  source: Source;
  /** The `(` the reference is inside, -1 at the top level. */
  parent: number;
  /** Index of the name's last token, to tell the alias position after it. */
  nameEnd: number;
  /** For a subquery: its parentheses. */
  body?: [number, number];
  /** For an INSERT target: the characters of the query giving it its rows, where it is not in
   *  scope. */
  hidden?: [number, number];
}

class Statement {
  readonly tokens: Token[];
  readonly ctes: Cte[] = [];
  readonly refs: Ref[] = [];
  /** `(` of the CTE bodies and the subqueries in FROM, whose names do not leak out. */
  readonly closed = new Set<number>();
  /** `(` around joined tables, `FROM (a JOIN b ON ...)`: their tables belong to the query. */
  private readonly groups = new Set<number>();

  private readonly catalog: Catalog;

  constructor(sql: string, catalog: Catalog, options: ScopeOptions) {
    this.catalog = catalog;
    this.tokens = tokenize(sql, options);
    this.parseCtes();
    this.parseRefs();
  }

  private parseCtes() {
    const t = this.tokens;
    for (let i = 0; i < t.length; i++) {
      if (!isWord(t[i], "WITH")) continue;
      let k = i + 1;
      if (isWord(t[k], "RECURSIVE")) k++;
      while (isName(t[k])) {
        const name = unquote(t[k]!.text);
        k++;
        let columns: string[] | null = null;
        if (t[k]?.text === "(") {
          const end = closing(t, k);
          columns = t.slice(k + 1, end).filter((x) => isName(x)).map((x) => unquote(x.text));
          k = end + 1;
        }
        if (!isWord(t[k], "AS")) break;
        k++;
        if (isWord(t[k], "NOT")) k++;
        if (isWord(t[k], "MATERIALIZED")) k++;
        if (t[k]?.text !== "(") break;
        const close = closing(t, k);
        this.ctes.push({ name, columns, open: k, close });
        this.closed.add(k);
        k = close + 1;
        if (t[k]?.text !== ",") break;
        k++;
      }
      i = k - 1;
    }
  }

  /** The name at `i` (`a`, `a.b` or `a.b.c`): its parts and the index after it. */
  private name(i: number): { parts: string[]; next: number } | null {
    const t = this.tokens;
    if (!isAliasable(t[i])) return null;
    const parts = [unquote(t[i]!.text)];
    let k = i + 1;
    while (t[k]?.text === "." && isName(t[k + 1]) && parts.length < 3) {
      parts.push(unquote(t[k + 1]!.text));
      k += 2;
    }
    return { parts, next: k };
  }

  /** `[AS] alias` at `i`: the alias, its token, and the index after it. */
  private alias(i: number): { alias: string | null; written?: string; next: number } {
    const t = this.tokens;
    const as = isWord(t[i], "AS");
    const k = as ? i + 1 : i;
    return isAliasable(t[k]) ? { alias: unquote(t[k]!.text), written: t[k]!.text, next: k + 1 } : { alias: null, next: i };
  }

  /** One item of FROM, JOIN, UPDATE or INTO at `i`; the index after it. */
  private item(i: number): number {
    const t = this.tokens;
    let lateral = false;
    for (; isWord(t[i], "ONLY", "LATERAL"); i++) lateral ||= isWord(t[i], "LATERAL");
    // UPDATE OR REPLACE t, in SQLite.
    if (isWord(t[i], "OR") && isName(t[i + 1])) i += 2;
    if (t[i]?.text === "(" && isWord(t[i + 1], "SELECT", "WITH", "VALUES")) {
      const close = closing(t, i);
      const { alias, written, next } = this.alias(close + 1);
      // A LATERAL subquery sees the tables before it.
      if (!lateral) this.closed.add(i);
      if (alias) {
        const source = { name: alias, written, schema: null, table: null, columns: [] };
        this.refs.push({ source, parent: this.parentOf(i), nameEnd: close, body: [i, close] });
      }
      return next;
    }
    if (t[i]?.text === "(") {
      this.groups.add(i);
      return this.item(i + 1);
    }
    const name = this.name(i);
    if (!name) return i;
    // A function in FROM, e.g. generate_series(...): no columns known.
    if (t[name.next]?.text === "(" && !isWord(t[i - 1], "INTO")) return closing(t, name.next) + 1;
    const table = name.parts.at(-1)!;
    const schema = name.parts.length > 1 ? name.parts.at(-2)! : null;
    const { alias, written, next } = this.alias(name.next);
    const cte = schema === null ? this.ctes.find((c) => same(c.name, table)) : undefined;
    const found = cte ? null : catalogTable(this.catalog, schema, table);
    const source: Source = {
      name: alias ?? table,
      written: written ?? t[name.next - 1]!.text,
      schema: found?.schema ?? null,
      table: found?.table ?? null,
      columns: cte?.columns ?? found?.columns ?? [],
    };
    const ref: Ref = { source, parent: this.parentOf(i), nameEnd: name.next - 1 };
    if (cte) ref.body = [cte.open, cte.close];
    this.refs.push(ref);
    return next;
  }

  /** The `(` of the query the token at `i` is in: around joined tables, the one outside. */
  private parentOf(i: number): number {
    let parent = this.tokens[i]!.parent;
    while (this.groups.has(parent)) parent = this.tokens[parent]!.parent;
    return parent;
  }

  private parseRefs() {
    const t = this.tokens;
    for (let i = 0; i < t.length; i++) {
      // FROM and USING take a list of tables, the others one.
      const list = isWord(t[i], "FROM") || (isWord(t[i], "USING") && this.usingTables(i));
      if (!list && !isWord(t[i], "JOIN", "UPDATE", "INTO")) continue;
      const first = this.refs.length;
      let k = this.item(i + 1);
      while (list && t[k]?.text === "," && t[k]!.parent === t[i]!.parent) k = this.item(k + 1);
      const target = this.refs[first];
      if (target && isWord(t[i], "INTO") && INSERTS.has(this.nearest(i, INTO_OWNERS) ?? "")) {
        target.hidden = this.insertRows(k, t[i]!.parent);
      }
    }
  }

  /** The characters of the query that gives an INSERT its rows: from token `k` (after the
   *  target) past the column list, up to ON CONFLICT, ON DUPLICATE KEY or RETURNING at `level`.
   *  Undefined when the rows are no query (`DEFAULT VALUES`, MySQL's `SET`). */
  private insertRows(k: number, level: number): [number, number] | undefined {
    const t = this.tokens;
    const query = (i: number) => isWord(t[i], "SELECT", "WITH", "VALUES");
    if (t[k]?.text === "(" && !query(k + 1)) k = closing(t, k) + 1;
    if (isWord(t[k], "OVERRIDING")) k += 3;
    if (!query(k) && t[k]?.text !== "(") return undefined;
    const limit = level < 0 ? t.length : closing(t, level);
    const ends = (i: number) =>
      t[i]!.parent === level && (isWord(t[i], "RETURNING") || (isWord(t[i], "ON") && isWord(t[i + 1], "CONFLICT", "DUPLICATE")));
    let end = k;
    while (end < limit && !ends(end)) end++;
    return [t[k]!.from, end < t.length ? t[end]!.from : Infinity];
  }

  /** Whether the `USING` at `i` names tables: in `DELETE … USING` and `MERGE … USING`, not in
   *  `JOIN … USING (column)` or in DDL (`USING heap`, `TYPE int USING x::int`). */
  private usingTables(i: number): boolean {
    const owner = this.nearest(i, USING_OWNERS);
    return owner === "DELETE" || owner === "MERGE";
  }

  /** The nearest of `words` at or before token `i`, at its level, upper-cased; null if none. */
  private nearest(i: number, words: Set<string>): string | null {
    const t = this.tokens;
    for (let k = i; k >= 0; k--) {
      if (t[k]!.parent !== t[i]!.parent || t[k]!.kind !== "word") continue;
      const w = t[k]!.text.toUpperCase();
      if (words.has(w)) return w;
    }
    return null;
  }

  /** The `(` around token position `pos` (a character offset), innermost first. */
  parentsAt(pos: number): number[] {
    const t = this.tokens;
    const stack: number[] = [];
    for (let i = 0; i < t.length && t[i]!.from < pos; i++) {
      if (t[i]!.text === "(") stack.push(i);
      else if (t[i]!.text === ")" && t[i]!.to <= pos) stack.pop();
    }
    return stack.reverse();
  }

  /** The sources visible at `pos`: those of its query and of the queries around it, up to
   *  the CTE body or the subquery in FROM it is in. */
  sourcesAt(pos: number, depth = 0): Source[] {
    const chain = this.parentsAt(pos);
    const limit = chain.findIndex((p) => this.closed.has(p));
    const visible = new Set(limit < 0 ? chain : chain.slice(0, limit + 1));
    if (limit < 0) visible.add(-1);
    const inScope = (r: Ref) => visible.has(r.parent) && !(r.hidden && pos >= r.hidden[0] && pos < r.hidden[1]);
    return this.refs.filter(inScope).map((r) => this.withColumns(r, depth));
  }

  private withColumns(ref: Ref, depth: number): Source {
    if (!ref.body || ref.source.columns.length || depth > 4) return ref.source;
    const [open, close] = ref.body;
    return { ...ref.source, columns: this.selectList(open, close, depth + 1) };
  }

  /** The names of the columns the query in `open..close` returns. */
  private selectList(open: number, close: number, depth: number): string[] {
    const t = this.tokens;
    let i = open + 1;
    while (i < close && !(isWord(t[i], "SELECT") && t[i]!.parent === open)) i++;
    if (i >= close) return [];
    i++;
    if (isWord(t[i], "DISTINCT", "ALL")) i++;
    if (isWord(t[i], "ON") && t[i + 1]?.text === "(") i = closing(t, i + 1) + 1;
    if (isWord(t[i], "TOP")) i += t[i + 1]?.text === "(" ? closing(t, i + 1) - i + 1 : 2;
    const items: Token[][] = [[]];
    for (; i < close; i++) {
      const tok = t[i]!;
      if (tok.parent === open && tok.kind === "word" && END_SELECT.has(tok.text.toUpperCase())) break;
      if (tok.parent === open && tok.text === ",") items.push([]);
      else if (tok.parent >= open) items.at(-1)!.push(tok);
    }
    const inner = () => this.sourcesAt(t[open]!.to, depth);
    const columns: string[] = [];
    for (const item of items) {
      // `x::type` is named after x, as PostgreSQL does.
      while (item.length > 3 && item.at(-2)?.text === ":" && item.at(-3)?.text === ":" && item.at(-1)?.kind === "word") {
        item.splice(-3);
      }
      const last = item.at(-1);
      const before = item.at(-2);
      if (!last) continue;
      if (last.text === "*") {
        const qualifier = item.length >= 3 && before?.text === "." ? unquote(item.at(-3)!.text) : null;
        for (const s of inner()) if (qualifier === null || same(s.name, qualifier)) columns.push(...s.columns);
      } else if (
        isName(last) &&
        !isWord(last, "END", "NULL", "TRUE", "FALSE") &&
        // A name alone or qualified, or an alias after an expression (`count(*) n`, `1 n`).
        (item.length === 1 || before?.text === "." || isName(before) || before?.text === ")" || /^\d$/.test(before?.text ?? ""))
      ) {
        columns.push(unquote(last.text));
      }
    }
    return columns;
  }

  /** The source named `name`, among those visible at `pos`. */
  sourceNamed(pos: number, name: string): Source | undefined {
    // A name the catalog does not know, e.g. the schema of `FROM sales.|`, is no source.
    const sources = this.sourcesAt(pos).filter((s) => s.table || s.columns.length);
    return byName(sources, name, (s) => s.name) ?? this.cteNamed(name);
  }

  /** The CTE named `name` as a source, for `x.` before `FROM x` is written. */
  private cteNamed(name: string): Source | undefined {
    const cte = byName(this.ctes, name, (c) => c.name);
    if (!cte) return undefined;
    const columns = cte.columns ?? this.selectList(cte.open, cte.close, 1);
    return { name: cte.name, schema: null, table: null, columns };
  }

  /** Index of the last token ending at or before `pos`. */
  tokenBefore(pos: number): number {
    let i = -1;
    while (i + 1 < this.tokens.length && this.tokens[i + 1]!.to <= pos) i++;
    return i;
  }

  /** Whether the cursor after token `i` names a table: after FROM, JOIN... or a comma of FROM. */
  tablePosition(i: number): boolean {
    const prev = this.tokens[i];
    if (!prev) return false;
    if (prev.kind === "word" && TABLE_KEYWORDS.has(prev.text.toUpperCase())) return true;
    return prev.text === "," && this.nearest(i, CLAUSES) === "FROM";
  }

  /** Whether the cursor after token `i` names a target column, which is written without its
   *  table: `SET |`, `SET a = 1, |`, `INSERT INTO t (|`, `USING (|`, `ON CONFLICT (|`. */
  targetPosition(i: number): boolean {
    const t = this.tokens;
    const prev = t[i];
    if (!prev || !(isWord(prev, "SET") || prev.text === "," || prev.text === "(")) return false;
    if (prev.text !== "(" && this.nearest(i, CLAUSES) === "SET") return true;
    let k = (prev.text === "(" ? i : prev.parent) - 1;
    if (t[k + 1]?.text !== "(") return false;
    if (isWord(t[k], "INSERT", "USING", "CONFLICT")) return true;
    // The column list of `INTO schema.table (`.
    while (isName(t[k]) && t[k - 1]?.text === ".") k -= 2;
    return isName(t[k]) && isWord(t[k - 1], "INTO");
  }

  /** Whether the cursor after token `i` names an alias: after `AS`, or after a table. */
  aliasPosition(i: number): boolean {
    return isWord(this.tokens[i], "AS") || this.refs.some((r) => r.nameEnd === i);
  }
}

const NAME_PART = String.raw`(?:"(?:[^"]|"")*"|\x60(?:[^\x60]|\x60\x60)*\x60|\[[^\]]*\]|[A-Za-z_\u0080-￿][\w$#\u0080-￿]*)`;
const QUALIFIER = new RegExp(String.raw`(${NAME_PART})\s*\.\s*$`);

/** The typed name before `pos`: where it starts, its opening quote, and the qualifiers
 *  written before it (`a.b.` gives `["a", "b"]`). */
function typedName(sql: string, pos: number, options: ScopeOptions) {
  // Strings and comments blanked: a quote in them opens no name.
  const before = maskSql(sql.slice(0, pos), options);
  const open = openName(before, options);
  const prefix = open >= 0 ? before.slice(open) : (/[\w$#\u0080-￿]*$/.exec(before)?.[0] ?? "");
  const from = pos - prefix.length;
  const path: string[] = [];
  for (let rest = before.slice(0, from); path.length < 3; ) {
    const m = QUALIFIER.exec(rest);
    if (!m) break;
    path.unshift(unquote(m[1]!));
    rest = rest.slice(0, m.index);
  }
  return { from, prefix, quote: open >= 0 ? prefix[0]! : null, path };
}

/** Where a quoted name still open at the end of `text` (masked) starts, or -1. */
function openName(text: string, options: ScopeOptions): number {
  for (let i = 0; i < text.length; i++) {
    const quote = quoteAt(text[i]!, options);
    if (!quote?.name) continue;
    const end = quotedEnd(text, i, quote.close, backslashIn(quote, options));
    if (end < 0) return i;
    i = end - 1;
  }
  return -1;
}

function tablesOf(catalog: Catalog, schema: string): NameItem[] {
  return Object.keys(catalog.schemas[schema] ?? {}).map((table) => ({ label: table, kind: "table", schema, table }));
}

/** The columns of `source`; `shared` holds the names (lower case) to insert qualified. */
function columnsOf(source: Source, qualified: boolean, shared?: Set<string>): NameItem[] {
  return source.columns.map((column) => ({
    label: column,
    kind: "column",
    ...(source.table ? { schema: source.schema!, table: source.table } : {}),
    ...(qualified ? {} : { source: source.name }),
    ...(shared?.has(column.toLowerCase()) ? { qualifier: source.written ?? source.name } : {}),
  }));
}

/** The column names (lower case) that more than one of `sources` has: ambiguous unqualified. */
function sharedColumns(sources: Source[]): Set<string> {
  const seen = new Set<string>();
  const shared = new Set<string>();
  for (const source of sources) {
    const names = new Set(source.columns.map((c) => c.toLowerCase()));
    for (const name of names) (seen.has(name) ? shared : seen).add(name);
  }
  return shared;
}

/** The names to offer at `pos` in `sql`, one statement; null where no name is written,
 *  e.g. an alias, or with nothing typed when `explicit` is false. */
export function nameOptions(
  sql: string,
  pos: number,
  catalog: Catalog,
  explicit: boolean,
  options: ScopeOptions = {},
): NameOptions | null {
  const typed = typedName(sql, pos, options);
  // The partly typed name would be read as part of the statement, and so would the quote the
  // editor closed after it, which would open a name taking the rest of the statement.
  const end = typed.quote && sql[pos] === closingQuote(typed.quote) ? pos + 1 : pos;
  const statement = new Statement(sql.slice(0, typed.from) + " ".repeat(end - typed.from) + sql.slice(end), catalog, options);
  const result = (items: NameItem[], unqualified = false): NameOptions | null =>
    items.length ? { from: typed.from, quote: typed.quote, unqualified, items } : null;

  if (typed.path.length) {
    if (typed.path.length === 1) {
      const first = typed.path[0]!;
      const source = statement.sourceNamed(pos, first);
      if (source) return result(columnsOf(source, true));
      const schema = schemaNamed(catalog, first);
      if (schema !== undefined) return result(tablesOf(catalog, schema));
      const table = catalogTable(catalog, null, first);
      return table ? result(columnsOf({ name: first, ...table }, true)) : null;
    }
    // `schema.table.`, or `database.schema.table.`.
    const table = catalogTable(catalog, typed.path.at(-2)!, typed.path.at(-1)!);
    return table ? result(columnsOf({ name: table.table, ...table }, true)) : null;
  }

  if (!typed.prefix && !explicit) return null;
  const prev = statement.tokenBefore(typed.from);
  if (statement.aliasPosition(prev)) return null;
  const schemas = Object.keys(catalog.schemas);
  const tables = catalog.default_schema
    ? tablesOf(catalog, catalog.default_schema)
    : schemas.length === 1
      ? tablesOf(catalog, schemas[0]!)
      : [];
  const schemaItems: NameItem[] = schemas.map((s) => ({ label: s, kind: "schema" }));
  const cteItems: NameItem[] = statement.ctes.map((c) => ({ label: c.name, kind: "source" }));
  if (statement.tablePosition(prev)) return result([...cteItems, ...tables, ...schemaItems]);

  // A name left blank by the typed text is no source.
  const sources = statement.sourcesAt(pos).filter((s) => s.name.trim() !== "");
  // A name several sources have is ambiguous alone, except where the column is a target.
  const shared = statement.targetPosition(prev) ? undefined : sharedColumns(sources);
  const columns = sources.flatMap((s) => columnsOf(s, false, shared));
  const named = sources
    .filter((s) => !s.table || !tables.some((t) => t.label === s.name))
    .map((s): NameItem => ({ label: s.name, kind: "source" }));
  return result([...columns, ...named, ...tables, ...schemaItems], true);
}
