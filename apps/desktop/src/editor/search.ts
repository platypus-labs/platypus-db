// The editor's language for Elasticsearch and OpenSearch: requests as Kibana's console writes
// them (`GET /index/_search`, then a JSON body on the lines below, ended by a blank line) and
// SQL statements (ended by a `;` or a blank line). Highlighting follows each statement's kind;
// completion offers the HTTP methods, the indices (the tables of the catalog), the APIs after an
// index, the fields of the index where a key of the body goes, and the same names in SQL.
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import { LanguageSupport, StreamLanguage, type StringStream } from "@codemirror/language";
import type { CompletionCatalog } from "../api";

const METHODS = ["GET", "POST", "PUT", "DELETE", "HEAD"];
const METHOD_LINE = /^\s*(GET|POST|PUT|DELETE|HEAD)\s+(?!from\b)\S/i;
const COMMENT = /^\s*(#|\/\/|--)/;

const api = (label: string, info: string): Completion => ({ label, type: "function", info });

/** The APIs after an index, `/index/_search`. */
const INDEX_APIS: Completion[] = [
  api("_search", "Searches the documents with a query."),
  api("_count", "Counts the documents that match a query."),
  api("_doc", "Reads, writes or deletes a document by its id."),
  api("_create", "Creates a document, refusing an id that exists."),
  api("_update", "Changes fields of a document."),
  api("_update_by_query", "Changes the documents that match a query."),
  api("_delete_by_query", "Deletes the documents that match a query."),
  api("_bulk", "Several writes in one request, a JSON line each."),
  api("_mget", "Several documents by their ids."),
  api("_mapping", "The fields of the index and their types."),
  api("_settings", "The settings of the index."),
  api("_refresh", "Makes the latest writes searchable."),
  api("_field_caps", "The capabilities of the fields."),
  api("_analyze", "Shows how a text is split into terms."),
  api("_explain", "Why a document matches, or not."),
];

/** The APIs of the cluster, `/_cat/indices`. */
const CLUSTER_APIS: Completion[] = [
  api("_search", "Searches every index."),
  api("_cat/indices?v", "The indices, with their sizes and document counts."),
  api("_cat/aliases?v", "The aliases and the indices they point to."),
  api("_cat/health?v", "The health of the cluster."),
  api("_cluster/health", "The health of the cluster, as JSON."),
  api("_cluster/stats", "Statistics of the whole cluster."),
  api("_nodes", "Information on the nodes."),
  api("_aliases", "Adds or removes aliases."),
  api("_bulk", "Several writes in one request, a JSON line each."),
  api("_msearch", "Several searches in one request, a JSON pair of lines each."),
  api("_sql", "Runs an SQL statement, in a JSON body: {\"query\": \"SELECT …\"}."),
];

const dsl = (label: string, info: string): Completion => ({ label, type: "keyword", info });

/** The keys of a search body and of its queries. */
const DSL: Completion[] = [
  dsl("query", "The query that picks the documents."),
  dsl("size", "How many hits to return."),
  dsl("from", "How many hits to skip."),
  dsl("sort", "The order of the hits."),
  dsl("_source", "The fields of each document to return."),
  dsl("aggs", "Aggregations: buckets and metrics over the hits."),
  dsl("track_total_hits", "Counts every match, not only the first 10 000."),
  dsl("match_all", "Every document."),
  dsl("match", "Full-text match of a field."),
  dsl("match_phrase", "A phrase, in order."),
  dsl("multi_match", "Full-text match of several fields."),
  dsl("term", "An exact value of a keyword, number or date field."),
  dsl("terms", "Any of several exact values."),
  dsl("range", "A range of numbers or dates: gte, gt, lte, lt."),
  dsl("exists", "Documents that have the field."),
  dsl("prefix", "Values that start with a text."),
  dsl("wildcard", "Values that match a pattern with * and ?."),
  dsl("ids", "Documents by their ids."),
  dsl("bool", "Combines queries: must, filter, should, must_not."),
  dsl("must", "Queries every hit matches, and that score."),
  dsl("filter", "Queries every hit matches, without scoring."),
  dsl("should", "Queries that raise the score, or one of which must match."),
  dsl("must_not", "Queries no hit matches."),
  dsl("date_range", "Buckets by ranges of dates."),
  dsl("date_histogram", "Buckets by a time interval."),
  dsl("avg", "The average of a field."),
  dsl("sum", "The sum of a field."),
  dsl("min", "The least value of a field."),
  dsl("max", "The greatest value of a field."),
  dsl("script", "A script that changes the document."),
  dsl("doc", "The fields to merge into a document."),
];

const SQL_KEYWORDS = [
  "SELECT", "FROM", "WHERE", "GROUP BY", "ORDER BY", "HAVING", "LIMIT", "AND", "OR", "NOT", "IN", "LIKE", "BETWEEN",
  "IS NULL", "IS NOT NULL", "AS", "DISTINCT", "ASC", "DESC", "SHOW TABLES", "SHOW COLUMNS", "DESCRIBE", "MATCH", "QUERY",
];
const SQL_WORDS = new Set(SQL_KEYWORDS.flatMap((k) => k.split(" ")));

type Mode = "none" | "request" | "body" | "sql";

/** The highlighting: a request line's method and path, the JSON of a body, the SQL. */
const tokens = StreamLanguage.define<{ mode: Mode }>({
  name: "elasticsearch",
  startState: () => ({ mode: "none" }),
  token(stream: StringStream, state) {
    if (stream.sol()) {
      if (stream.match(/^\s*$/, false)) {
        state.mode = "none";
        stream.skipToEnd();
        return null;
      }
      if (state.mode === "request") state.mode = "body";
      if (COMMENT.test(stream.string)) {
        stream.skipToEnd();
        return "lineComment";
      }
      if (state.mode !== "body" && METHOD_LINE.test(stream.string)) {
        state.mode = "request";
        stream.eatSpace();
        stream.eatWhile(/\w/);
        return "keyword";
      }
      if (state.mode === "none") state.mode = "sql";
    }
    if (stream.eatSpace()) return null;
    if (state.mode === "request") {
      stream.eatWhile(/\S/);
      return "atom";
    }
    if (state.mode === "body") return bodyToken(stream);
    return sqlToken(stream);
  },
  // Empty lines never reach `token`: they end a request or a statement all the same.
  blankLine(state) {
    state.mode = "none";
  },
  languageData: { commentTokens: { line: "#" } },
});

function bodyToken(stream: StringStream): string | null {
  const c = stream.next();
  if (c === '"') {
    let escaped = false;
    for (let d = stream.next(); d !== undefined; d = stream.next()) {
      if (d === '"' && !escaped) break;
      escaped = !escaped && d === "\\";
    }
    return stream.match(/^\s*:/, false) ? "propertyName" : "string";
  }
  if (c !== undefined && /[-\d]/.test(c)) {
    stream.eatWhile(/[\d.eE+-]/);
    return "number";
  }
  if (c !== undefined && /[a-z]/.test(c)) {
    stream.eatWhile(/[a-z]/);
    return /^(true|false|null)$/.test(stream.current()) ? "atom" : null;
  }
  return null;
}

function sqlToken(stream: StringStream): string | null {
  if (stream.match("--")) {
    stream.skipToEnd();
    return "lineComment";
  }
  const c = stream.next();
  if (c === "'" || c === '"') {
    for (let d = stream.next(); d !== undefined; d = stream.next()) if (d === c) break;
    return c === "'" ? "string" : "variableName";
  }
  if (c !== undefined && /\d/.test(c)) {
    stream.eatWhile(/[\d.]/);
    return "number";
  }
  if (c !== undefined && /[\w@]/.test(c)) {
    stream.eatWhile(/[\w.@*-]/);
    return SQL_WORDS.has(stream.current().toUpperCase()) ? "keyword" : null;
  }
  return null;
}

/** The names of the catalog: index → fields. */
function indices(catalog: CompletionCatalog | null | undefined): Record<string, string[]> {
  if (!catalog) return {};
  const schemas = catalog.default_schema ? [catalog.default_schema] : Object.keys(catalog.schemas);
  return Object.assign({}, ...schemas.map((s) => catalog.schemas[s] ?? {})) as Record<string, string[]>;
}

/** What the statement around `pos` is, and its first line: the request line the body follows. */
function statementAt(context: CompletionContext): { kind: "request" | "body" | "sql"; head: string } {
  const doc = context.state.doc;
  const current = doc.lineAt(context.pos).number;
  let first = current;
  for (let n = current - 1; n >= 1 && doc.line(n).text.trim() !== ""; n--) first = n;
  // Comments before the statement's first line.
  while (first < current && COMMENT.test(doc.line(first).text)) first++;
  const head = doc.line(first).text;
  if (!METHOD_LINE.test(head)) return { kind: "sql", head };
  return { kind: first === current ? "request" : "body", head };
}

const NAME = /^[\w.@*:-]*$/;

function completeSearch(catalog: CompletionCatalog | null | undefined) {
  return (context: CompletionContext): CompletionResult | null => {
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    const names = indices(catalog);
    const index = (label: string): Completion => ({ label, type: "class", boost: 1 });
    const { kind, head } = statementAt(context);

    if (kind === "request") {
      const method = /^\s*[A-Za-z]*$/.exec(before);
      if (method) {
        const word = context.matchBefore(/[A-Za-z]*/);
        if (!context.explicit && word?.from === word?.to) return null;
        const start = line.from + before.length - before.trimStart().length;
        return { from: start, options: METHODS.map((label): Completion => ({ label, type: "keyword", apply: `${label} /` })) };
      }
      // `/index/_api`: the APIs after an index; the indices and the cluster's APIs before.
      const target = /^\s*[A-Za-z]+\s+\/?([^\s?]*)$/.exec(before);
      if (!target) return null;
      const path = target[1] ?? "";
      const slash = path.indexOf("/");
      const from = context.pos - (slash < 0 ? path.length : path.length - slash - 1);
      if (slash < 0) {
        const options = [...Object.keys(names).map(index), ...CLUSTER_APIS];
        return { from, options, validFor: NAME };
      }
      return { from, options: INDEX_APIS, validFor: NAME };
    }

    if (kind === "body") {
      // A key of an object: a query keyword, or a field of the index the request names.
      if (!/[{,]\s*"?[\w.@$-]*$/.test(before) && !/^\s*"?[\w.@$-]*$/.test(before)) return null;
      const word = context.matchBefore(/[\w.@$-]*/);
      if (!word || (word.from === word.to && !context.explicit)) return null;
      const named = /^\s*\w+\s+\/?([^\s/?]+)/.exec(head)?.[1];
      const fields = named && names[named] ? names[named] : [...new Set(Object.values(names).flat())];
      const options = [...fields.map((f): Completion => ({ label: f, type: "property", boost: 1 })), ...DSL];
      return { from: word.from, options, validFor: NAME };
    }

    // SQL: an index after FROM or JOIN, else the fields, then the keywords.
    const word = context.matchBefore(/[\w.@*-]*/);
    if (!word || (word.from === word.to && !context.explicit)) return null;
    if (/\b(from|join|describe|columns\s+from)\s+"?[\w.@*-]*$/i.test(before)) {
      return { from: word.from, options: Object.keys(names).map(index), validFor: NAME };
    }
    const fields = [...new Set(Object.values(names).flat())];
    // The first word of a statement may be the method of a request still to be written.
    const methods =
      head === line.text && /^\s*[A-Za-z]*$/.test(before)
        ? METHODS.map((label): Completion => ({ label, type: "keyword", apply: `${label} /`, boost: 1 }))
        : [];
    return {
      from: word.from,
      options: [
        ...methods,
        ...fields.map((f): Completion => ({ label: f, type: "property" })),
        ...SQL_KEYWORDS.map((label): Completion => ({ label, type: "keyword" })),
      ],
      validFor: NAME,
    };
  };
}

/** Elasticsearch highlighting and completion for a search editor. */
export function searchLanguage(catalog: CompletionCatalog | null | undefined) {
  return new LanguageSupport(tokens, [tokens.data.of({ autocomplete: completeSearch(catalog) })]);
}
