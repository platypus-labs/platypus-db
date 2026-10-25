// JOIN completion from foreign keys. After `JOIN ` it offers the tables related to the ones
// already in the query, with alias and condition (`customers c ON c.id = o.customer_id`).
// After `JOIN t ON ` it offers the condition between `t` and those tables. The SQL before
// the cursor is read with regular expressions: enough for the FROM and JOIN clauses.
import type { Completion, CompletionContext, CompletionResult, CompletionSource } from "@codemirror/autocomplete";
import type { CatalogForeignKey, CompletionCatalog, DriverKind } from "../api";
import { quoteIdent } from "../sqlNames";
import { t } from "../i18n";

interface TableRef {
  schema: string | null;
  table: string;
  /** The name as written, used to qualify columns when there is no alias. */
  text: string;
  alias: string | null;
}

const IDENT = '(?:"(?:[^"]|"")+"|`(?:[^`]|``)+`|[A-Za-z_][\\w$]*)';
const NAME = `${IDENT}(?:\\s*\\.\\s*${IDENT})?`;
/** What may follow a table without being its alias. */
const NOT_ALIAS = new Set(
  (
    "ON USING WHERE JOIN LEFT RIGHT INNER OUTER FULL CROSS NATURAL GROUP ORDER LIMIT OFFSET " +
    "HAVING UNION INTERSECT EXCEPT SET WINDOW LATERAL AS VALUES SELECT FOR RETURNING"
  ).split(" "),
);
/** ` alias` or ` AS alias` after a table name, never a keyword. */
const ALIAS = `(?:\\s+(?:AS\\s+)?(?!(?:${[...NOT_ALIAS].join("|")})\\b)(${IDENT}))?`;
/** Aliases that would read as keywords. */
const NOT_GENERATED = new Set(["as", "at", "by", "do", "if", "in", "is", "no", "of", "on", "or", "to"]);

function unquote(ident: string): string {
  const q = ident[0];
  if (q === '"' || q === "`") return ident.slice(1, -1).replaceAll(q + q, q);
  return ident;
}

function parseRef(name: string, alias: string | undefined): TableRef {
  const parts = name.match(new RegExp(IDENT, "g")) ?? [name];
  const usable = alias && !NOT_ALIAS.has(alias.toUpperCase()) ? alias : null;
  return {
    schema: parts.length > 1 ? unquote(parts[0]!) : null,
    table: unquote(parts[parts.length - 1]!),
    text: name.replace(/\s+/g, ""),
    alias: usable,
  };
}

/** Tables named after FROM, JOIN, UPDATE and INTO, in order. */
export function tableRefs(sql: string): TableRef[] {
  const re = new RegExp(`\\b(?:FROM|JOIN|UPDATE|INTO)\\s+(${NAME})${ALIAS}`, "gi");
  return [...sql.matchAll(re)].map((m) => parseRef(m[1]!, m[2]));
}

function sameName(a: string, b: string) {
  return a.toLowerCase() === b.toLowerCase();
}

function refers(ref: TableRef, schema: string, table: string, catalog: CompletionCatalog) {
  return sameName(ref.table, table) && sameName(ref.schema ?? catalog.default_schema ?? schema, schema);
}

/** `a.x = b.y AND ...` for the key from `from` (the referencing side) to `to`. */
function condition(fk: CatalogForeignKey, from: string, to: string, driver: DriverKind) {
  return fk.columns
    .map((c, i) => `${from}.${quoteIdent(c, driver)} = ${to}.${quoteIdent(fk.ref_columns[i] ?? c, driver)}`)
    .join(" AND ");
}

const qualifier = (ref: TableRef) => ref.alias ?? ref.text;

function alias(table: string, used: Set<string>): string {
  const base = table
    .split(/[_\s]+/)
    .filter(Boolean)
    .map((w) => w[0]!.toLowerCase())
    .join("") || "t";
  let name = base;
  for (let n = 2; used.has(name) || NOT_GENERATED.has(name); n++) name = `${base}${n}`;
  return name;
}

/** Conditions between `joined` and each table before it. */
function conditions(catalog: CompletionCatalog, driver: DriverKind, joined: TableRef, before: TableRef[]) {
  const options: Completion[] = [];
  for (const fk of catalog.foreign_keys) {
    for (const other of before) {
      if (refers(joined, fk.schema, fk.table, catalog) && refers(other, fk.ref_schema, fk.ref_table, catalog)) {
        options.push({ label: condition(fk, qualifier(joined), qualifier(other), driver) });
      } else if (refers(other, fk.schema, fk.table, catalog) && refers(joined, fk.ref_schema, fk.ref_table, catalog)) {
        options.push({ label: condition(fk, qualifier(other), qualifier(joined), driver) });
      }
    }
  }
  return options;
}

/** Tables related to `before` by a key, each with an alias and its condition. */
function relatedTables(catalog: CompletionCatalog, driver: DriverKind, before: TableRef[]) {
  const used = new Set(before.flatMap((r) => [r.alias ?? r.table].map((n) => n.toLowerCase())));
  const options = new Map<string, Completion>();
  const add = (schema: string, table: string, build: (alias: string) => string) => {
    const a = alias(table, used);
    const name = schema === catalog.default_schema ? quoteIdent(table, driver) : `${quoteIdent(schema, driver)}.${quoteIdent(table, driver)}`;
    const label = `${name} ${a} ON ${build(a)}`;
    if (!options.has(label)) options.set(label, { label, type: "type", detail: t("foreign key"), boost: 10 });
  };
  for (const fk of catalog.foreign_keys) {
    for (const other of before) {
      if (refers(other, fk.ref_schema, fk.ref_table, catalog)) {
        add(fk.schema, fk.table, (a) => condition(fk, a, qualifier(other), driver));
      }
      if (refers(other, fk.schema, fk.table, catalog)) {
        add(fk.ref_schema, fk.ref_table, (a) => condition(fk, qualifier(other), a, driver));
      }
    }
  }
  return [...options.values()];
}

const AFTER_ON = new RegExp(`\\bJOIN\\s+(${NAME})${ALIAS}\\s+ON\\s+([\\w$.]*)$`, "i");
const AFTER_JOIN = /\bJOIN\s+([\w$."`]*)$/i;

export function joinCompletion(catalog: CompletionCatalog | null | undefined, driver: DriverKind): CompletionSource {
  return (context: CompletionContext): CompletionResult | null => {
    if (!catalog?.foreign_keys?.length) return null;
    const text = context.state.sliceDoc(Math.max(0, context.pos - 4000), context.pos);
    const statement = text.slice(text.lastIndexOf(";") + 1);

    const on = AFTER_ON.exec(statement);
    if (on) {
      const options = conditions(catalog, driver, parseRef(on[1]!, on[2]), tableRefs(statement.slice(0, on.index)));
      if (!options.length) return null;
      return { from: context.pos - on[3]!.length, options, validFor: /^[\w$.]*$/ };
    }
    const join = AFTER_JOIN.exec(statement);
    if (join) {
      const options = relatedTables(catalog, driver, tableRefs(statement.slice(0, join.index)));
      if (!options.length) return null;
      return { from: context.pos - join[1]!.length, options };
    }
    return null;
  };
}
