// Names to complete at the cursor of one SQL statement: the columns of the tables it reads
// (before or after the cursor), the tables of a schema, the columns of `alias.`, of a CTE or
// of a subquery in FROM. Plain logic on the statement's text, without CodeMirror, so that it
// can be tested on its own (tests/sqlScope.test.mjs).
import type { CompletionCatalog } from "../api";

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
}

const WORD = /[A-Za-z_\u0080-￿][\w$#\u0080-￿]*/y;
/** Words after which a table is named. */
const TABLE_KEYWORDS = new Set(["FROM", "JOIN", "UPDATE", "INTO", "TABLE"]);
/** What may follow a table without being its alias. */
const NOT_ALIAS = new Set(
  (
    "ON USING WHERE JOIN LEFT RIGHT INNER OUTER FULL CROSS NATURAL GROUP ORDER LIMIT OFFSET " +
    "HAVING UNION INTERSECT EXCEPT MINUS SET WINDOW LATERAL AS VALUES SELECT FOR RETURNING DEFAULT " +
    "WITH FETCH QUALIFY TABLESAMPLE STRAIGHT_JOIN USE FORCE IGNORE INDEXED NOT WHEN OR AND"
  ).split(" "),
);
/** Words that end the select list. */
const END_SELECT = new Set(
  "FROM INTO WHERE GROUP HAVING ORDER LIMIT OFFSET FETCH UNION INTERSECT EXCEPT MINUS WINDOW QUALIFY".split(" "),
);

/** The SQL with strings and comments blanked out, the same length. */
export function maskSql(sql: string): string {
  let out = "";
  let i = 0;
  const blank = (s: string) => s.replace(/[^\n]/g, " ");
  while (i < sql.length) {
    const c = sql[i]!;
    let end = -1;
    let keep = false;
    if (c === "'") {
      end = i + 1;
      while (end < sql.length && !(sql[end] === "'" && sql[end + 1] !== "'")) end += sql[end] === "'" ? 2 : 1;
      end++;
    } else if (c === "-" && sql[i + 1] === "-") {
      end = sql.indexOf("\n", i);
      if (end < 0) end = sql.length;
    } else if (c === "/" && sql[i + 1] === "*") {
      end = sql.indexOf("*/", i + 2);
      end = end < 0 ? sql.length : end + 2;
    } else if (c === "$") {
      const tag = /^\$[A-Za-z_]*\$/.exec(sql.slice(i, i + 64))?.[0];
      if (tag) {
        end = sql.indexOf(tag, i + tag.length);
        end = end < 0 ? sql.length : end + tag.length;
      }
    } else if (c === '"' || c === "`") {
      // A quoted name stays, but a quote inside it starts no string.
      end = i + 1;
      while (end < sql.length && !(sql[end] === c && sql[end + 1] !== c)) end += sql[end] === c ? 2 : 1;
      end++;
      keep = true;
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
export function statementAt(doc: string, pos: number): { from: number; to: number } {
  const text = maskSql(doc);
  const end = text.indexOf(";", pos);
  return { from: text.lastIndexOf(";", pos - 1) + 1, to: end < 0 ? text.length : end };
}

function tokenize(sql: string, options: ScopeOptions): Token[] {
  const text = maskSql(sql);
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
    const close = c === '"' || c === "`" ? c : c === "[" && options.brackets ? "]" : null;
    if (close) {
      let end = i + 1;
      while (end < text.length && !(text[end] === close && text[end + 1] !== close)) end += text[end] === close ? 2 : 1;
      end = Math.min(end + 1, text.length);
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

/** A quoted name without its quotes. */
export function unquote(ident: string): string {
  const q = ident[0];
  const close = q === "[" ? "]" : q;
  if ((q === '"' || q === "`" || q === "[") && ident.length >= 2 && ident.endsWith(close!)) {
    return ident.slice(1, -1).replaceAll(close! + close!, close!);
  }
  return ident;
}

const isName = (t: Token | undefined) => !!t && (t.kind === "quoted" || t.kind === "word");
const isWord = (t: Token | undefined, ...words: string[]) => t?.kind === "word" && words.includes(t.text.toUpperCase());
const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** Index of the `)` closing the `(` at `open`, or `tokens.length`. */
function closing(tokens: Token[], open: number) {
  for (let i = open + 1; i < tokens.length; i++) {
    if (tokens[i]!.text === ")" && tokens[i]!.kind === "punct" && tokens[i]!.parent === tokens[open]!.parent) return i;
  }
  return tokens.length;
}

/** Looks up a table: in `schema`, or in the default schema and then in any other. */
function catalogTable(catalog: Catalog, schema: string | null, table: string): { schema: string; table: string; columns: string[] } | null {
  const find = (s: string) => {
    const tables = catalog.schemas[s];
    if (!tables) return null;
    const name = table in tables ? table : Object.keys(tables).find((t) => same(t, table));
    return name === undefined ? null : { schema: s, table: name, columns: tables[name]! };
  };
  const schemas = Object.keys(catalog.schemas);
  if (schema !== null) {
    const s = schema in catalog.schemas ? schema : schemas.find((n) => same(n, schema));
    return s === undefined ? null : find(s);
  }
  if (catalog.default_schema) {
    const found = find(catalog.default_schema);
    if (found) return found;
  }
  for (const s of schemas) {
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
}

class Statement {
  readonly tokens: Token[];
  readonly ctes: Cte[] = [];
  readonly refs: Ref[] = [];
  /** `(` of the CTE bodies and the subqueries in FROM, whose names do not leak out. */
  readonly closed = new Set<number>();

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
    if (!isName(t[i]) || (t[i]!.kind === "word" && NOT_ALIAS.has(t[i]!.text.toUpperCase()))) return null;
    const parts = [unquote(t[i]!.text)];
    let k = i + 1;
    while (t[k]?.text === "." && isName(t[k + 1]) && parts.length < 3) {
      parts.push(unquote(t[k + 1]!.text));
      k += 2;
    }
    return { parts, next: k };
  }

  /** `[AS] alias` at `i`: the alias and the index after it. */
  private alias(i: number): { alias: string | null; next: number } {
    const t = this.tokens;
    const as = isWord(t[i], "AS");
    const k = as ? i + 1 : i;
    const tok = t[k];
    if (isName(tok) && !(tok!.kind === "word" && NOT_ALIAS.has(tok!.text.toUpperCase()))) {
      return { alias: unquote(tok!.text), next: k + 1 };
    }
    return { alias: null, next: i };
  }

  /** One item of FROM, JOIN, UPDATE or INTO at `i`; the index after it. */
  private item(i: number): number {
    const t = this.tokens;
    while (isWord(t[i], "ONLY", "LATERAL")) i++;
    // UPDATE OR REPLACE t, in SQLite.
    if (isWord(t[i], "OR") && isName(t[i + 1])) i += 2;
    if (t[i]?.text === "(" && isWord(t[i + 1], "SELECT", "WITH", "VALUES")) {
      const close = closing(t, i);
      const { alias, next } = this.alias(close + 1);
      this.closed.add(i);
      if (alias) {
        const source = { name: alias, schema: null, table: null, columns: [] };
        this.refs.push({ source, parent: t[i]!.parent, nameEnd: close, body: [i, close] });
      }
      return next;
    }
    const name = this.name(i);
    if (!name) return i;
    // A function in FROM, e.g. generate_series(...): no columns known.
    if (t[name.next]?.text === "(" && !isWord(t[i - 1], "INTO")) return closing(t, name.next) + 1;
    const table = name.parts.at(-1)!;
    const schema = name.parts.length > 1 ? name.parts.at(-2)! : null;
    const { alias, next } = this.alias(name.next);
    const cte = schema === null ? this.ctes.find((c) => same(c.name, table)) : undefined;
    const found = cte ? null : catalogTable(this.catalog, schema, table);
    const source: Source = {
      name: alias ?? table,
      schema: found?.schema ?? null,
      table: found?.table ?? null,
      columns: found?.columns ?? [],
    };
    const ref: Ref = { source, parent: t[i]!.parent, nameEnd: name.next - 1 };
    if (cte) ref.body = [cte.open, cte.close];
    if (cte?.columns) source.columns = cte.columns;
    this.refs.push(ref);
    return next;
  }

  private parseRefs() {
    const t = this.tokens;
    for (let i = 0; i < t.length; i++) {
      if (!isWord(t[i], "FROM", "JOIN", "UPDATE", "INTO")) continue;
      const from = isWord(t[i], "FROM");
      let k = this.item(i + 1);
      while (from && t[k]?.text === "," && t[k]!.parent === t[i]!.parent) k = this.item(k + 1);
    }
  }

  /** The `(` around token position `pos` (a character offset), innermost first. */
  parentsAt(pos: number): number[] {
    const t = this.tokens;
    const chain: number[] = [];
    const stack: number[] = [];
    for (let i = 0; i < t.length && t[i]!.from < pos; i++) {
      if (t[i]!.text === "(" && t[i]!.kind === "punct") stack.push(i);
      else if (t[i]!.text === ")" && t[i]!.kind === "punct" && t[i]!.to <= pos) stack.pop();
    }
    for (let i = stack.length - 1; i >= 0; i--) chain.push(stack[i]!);
    return chain;
  }

  /** The sources visible at `pos`: those of its query and of the queries around it, up to
   *  the CTE body or the subquery in FROM it is in. */
  sourcesAt(pos: number, depth = 0): Source[] {
    const chain = this.parentsAt(pos);
    const limit = chain.findIndex((p) => this.closed.has(p));
    const visible = new Set(limit < 0 ? chain : chain.slice(0, limit + 1));
    if (limit < 0) visible.add(-1);
    return this.refs.filter((r) => visible.has(r.parent)).map((r) => this.withColumns(r, depth));
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
      else if (tok.parent === open || tok.parent > open) items.at(-1)!.push(tok);
    }
    const inner = () => this.sourcesAt(t[open]!.to, depth);
    const columns: string[] = [];
    for (const item of items) {
      const last = item.at(-1);
      const before = item.at(-2);
      if (!last) continue;
      if (last.text === "*") {
        const qualifier = item.length >= 3 && before?.text === "." ? unquote(item.at(-3)!.text) : null;
        for (const s of inner()) if (qualifier === null || same(s.name, qualifier)) columns.push(...s.columns);
      } else if (
        isName(last) &&
        !isWord(last, "END", "NULL", "TRUE", "FALSE") &&
        (item.length === 1 || before?.text === "." || isName(before) || before?.text === ")")
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
    return sources.find((s) => s.name === name) ?? sources.find((s) => same(s.name, name));
  }

  /** Index of the last token ending at or before `pos`. */
  tokenBefore(pos: number): number {
    let i = -1;
    while (i + 1 < this.tokens.length && this.tokens[i + 1]!.to <= pos) i++;
    return i;
  }

  /** Whether the cursor after token `i` names a table: after FROM, JOIN... or a comma of FROM. */
  tablePosition(i: number): boolean {
    const t = this.tokens;
    const prev = t[i];
    if (!prev) return false;
    if (prev.kind === "word" && TABLE_KEYWORDS.has(prev.text.toUpperCase())) return true;
    if (prev.text !== ",") return false;
    for (let k = i - 1; k >= 0; k--) {
      if (t[k]!.parent !== prev.parent || t[k]!.kind !== "word") continue;
      const w = t[k]!.text.toUpperCase();
      if (w === "FROM") return true;
      if (END_SELECT.has(w) || w === "SELECT" || w === "SET" || w === "ON" || w === "JOIN") return false;
    }
    return false;
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
  const before = maskSql(sql.slice(0, pos));
  const quoted = options.brackets ? /(?:"[^"]*|\x60[^\x60]*|\[[^\]]*)$/ : /(?:"[^"]*|\x60[^\x60]*)$/;
  const open = quoted.exec(before);
  // An opening quote only when the text before has an even number of them.
  const isOpen = !!open && (before.slice(0, open.index).split(open[0][0]!).length - 1) % 2 === 0;
  const prefix = isOpen ? open![0] : (/[\w$#\u0080-￿]*$/.exec(before)?.[0] ?? "");
  const from = pos - prefix.length;
  const path: string[] = [];
  for (let rest = before.slice(0, from); path.length < 3; ) {
    const m = QUALIFIER.exec(rest);
    if (!m) break;
    path.unshift(unquote(m[1]!));
    rest = rest.slice(0, m.index);
  }
  return { from, prefix, quote: isOpen ? prefix[0]! : null, path };
}

function tablesOf(catalog: Catalog, schema: string): NameItem[] {
  return Object.keys(catalog.schemas[schema] ?? {}).map((table) => ({ label: table, kind: "table", schema, table }));
}

function schemaNamed(catalog: Catalog, name: string): string | undefined {
  const schemas = Object.keys(catalog.schemas);
  return name in catalog.schemas ? name : schemas.find((s) => same(s, name));
}

function columnsOf(source: Source, qualified: boolean): NameItem[] {
  return source.columns.map((column) => ({
    label: column,
    kind: "column",
    ...(source.table ? { schema: source.schema!, table: source.table } : {}),
    ...(qualified ? {} : { source: source.name }),
  }));
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
  // The partly typed name would be read as part of the statement.
  const statement = new Statement(sql.slice(0, typed.from) + " ".repeat(pos - typed.from) + sql.slice(pos), catalog, options);
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

  const sources = statement.sourcesAt(pos);
  const columns = sources.flatMap((s) => columnsOf(s, false));
  const named = sources
    .filter((s) => !s.table || !tables.some((t) => t.label === s.name))
    .map((s): NameItem => ({ label: s.name, kind: "source" }));
  return result([...columns, ...named, ...tables, ...schemaItems], true);
}
