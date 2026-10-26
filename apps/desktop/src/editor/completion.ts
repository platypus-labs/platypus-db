// The SQL editor's completion sources and keys: names from the catalog in the scope of the
// statement under the cursor (sqlScope.ts), keywords in the configured case, and the rules
// that keep the list out of strings and comments and away from a newline.
import {
  type Completion,
  type CompletionContext,
  type CompletionResult,
  type CompletionSource,
  closeCompletion,
  completionStatus,
  selectedCompletion,
} from "@codemirror/autocomplete";
import { keywordCompletionSource, type SQLDialect } from "@codemirror/lang-sql";
import { syntaxTree } from "@codemirror/language";
import type { EditorView } from "@codemirror/view";
import type { CompletionCatalog, DriverKind } from "../api";
import { getSetting } from "../settings";
import { quoteIdent } from "../sqlNames";
import { formatKeywordCase } from "./actions";
import { functionNames } from "./functions";
import { type NameItem, type NameOptions, nameOptions, statementAt } from "./sqlScope";

/** The note of a table, or of its column, shown beside the entry. */
export type CatalogNote = (schema: string, table: string, column?: string) => string | undefined;

const NOT_CODE = new Set(["String", "LineComment", "BlockComment", "QuotedIdentifier"]);

/** `source` everywhere but in strings and comments (and quoted names unless `quoted`). */
export function codeOnly(source: CompletionSource, quoted = false): CompletionSource {
  return (context) => {
    const tree = syntaxTree(context.state);
    for (let n = tree.resolveInner(context.pos, -1) as ReturnType<typeof tree.resolveInner> | null; n; n = n.parent) {
      if (NOT_CODE.has(n.name) && !(quoted && n.name === "QuotedIdentifier")) return null;
    }
    return source(context);
  };
}

const TYPE: Record<NameItem["kind"], string> = { column: "property", table: "type", schema: "namespace", source: "constant" };
/** How far around the cursor a long script is read for its statement. */
const WINDOW = 100_000;

function completion(item: NameItem, result: NameOptions, driver: DriverKind, note: CatalogNote | undefined): Completion {
  const info = item.table && note?.(item.schema!, item.table, item.kind === "column" ? item.label : undefined);
  const close = result.quote === "[" ? "]" : result.quote;
  // After a typed quote the name goes on inside it; otherwise it is quoted when it must be.
  const label = close ? `${result.quote}${item.label.replaceAll(close, close + close)}${close}` : item.label;
  const apply = close ? label : quoteIdent(item.label, driver);
  return {
    label,
    type: TYPE[item.kind],
    ...(apply !== label ? { apply } : {}),
    ...(item.source ? { detail: item.source } : {}),
    ...(info ? { info } : {}),
    // The columns in scope come before the keywords and the tables.
    ...(result.unqualified && item.kind === "column" ? { boost: 1 } : {}),
  };
}

/** Tables, schemas and the columns in the scope of the statement under the cursor. */
export function nameCompletion(catalog: CompletionCatalog, driver: DriverKind, note?: CatalogNote): CompletionSource {
  const options = { brackets: driver === "sqlserver" };
  return codeOnly((context: CompletionContext): CompletionResult | null => {
    const base = Math.max(0, context.pos - WINDOW);
    const text = context.state.sliceDoc(base, context.pos + WINDOW);
    const statement = statementAt(text, context.pos - base);
    const sql = text.slice(statement.from, statement.to);
    const result = nameOptions(sql, context.pos - base - statement.from, catalog, context.explicit, options);
    if (!result) return null;
    const close = result.quote === "[" ? "]" : result.quote;
    const quoteAfter = close !== null && context.state.sliceDoc(context.pos, context.pos + 1) === close;
    return {
      from: base + statement.from + result.from,
      to: quoteAfter ? context.pos + 1 : undefined,
      options: result.items.map((item) => completion(item, result, driver, note)),
      validFor: result.quote ? /^["`[][^"`\]]*["`\]]?$/ : /^[\w$#\u0080-￿]*$/,
    };
  }, true);
}

/** Keywords that are functions too but also start a clause or a type, kept as keywords. */
const KEYWORD_TOO = new Set(["left", "right", "replace", "if", "date"]);

/** The dialect's keywords, without those the function list has (with their docs), in the
 *  case of the setting: as typed follows the word being typed. */
export function keywordCompletion(dialect: SQLDialect, driver: DriverKind): CompletionSource {
  const functions = functionNames(driver);
  const filtered = new WeakMap<readonly Completion[], Completion[]>();
  const source = (upper: boolean): CompletionSource => {
    const keywords = keywordCompletionSource(dialect, upper);
    return (context) => {
      const result = keywords(context) as CompletionResult | null;
      if (!result) return null;
      let options = filtered.get(result.options);
      if (!options) {
        options = result.options.filter((o) => KEYWORD_TOO.has(o.label.toLowerCase()) || !functions.has(o.label.toLowerCase()));
        filtered.set(result.options, options);
      }
      return { ...result, options };
    };
  };
  const upper = source(true);
  const lower = source(false);
  return (context) => {
    const mode = getSetting(formatKeywordCase);
    const typed = context.matchBefore(/\w+/)?.text ?? "";
    const asLower = mode === "lower" || (mode === "preserve" && typed !== "" && typed === typed.toLowerCase());
    return (asLower ? lower : upper)(context);
  };
}

/** Enter on a word typed in full closes the list and goes to a new line instead of accepting
 *  the entry the word already is. Returns false so that Enter goes on to the next binding. */
export function closeOnTypedWord(view: EditorView): boolean {
  if (completionStatus(view.state) !== "active") return false;
  const selected = selectedCompletion(view.state);
  const head = view.state.selection.main.head;
  const word = /[\w$#]+$/.exec(view.state.sliceDoc(Math.max(0, head - 200), head))?.[0];
  if (selected && word && word.toLowerCase() === selected.label.toLowerCase()) closeCompletion(view);
  return false;
}
