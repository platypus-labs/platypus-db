// The editor's language for MongoDB: mongosh is JavaScript, with completion of the shell
// commands, the collections after `db.`, their methods and the fields of the collection a
// statement works on (from the sampled catalog).
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import { javascript } from "@codemirror/lang-javascript";
import type { CompletionCatalog } from "../api";

const method = (label: string, detail: string, info: string): Completion => ({
  label,
  type: "method",
  detail,
  info,
  apply: `${label}()`,
});

const DB_METHODS: Completion[] = [
  method("getCollectionNames", "()", "The names of the database's collections."),
  method("getCollectionInfos", "(filter?)", "The collections with their options."),
  method("getCollection", "(name)", "A collection whose name is not an identifier."),
  method("getSiblingDB", "(name)", "Another database of the server."),
  method("runCommand", "(command)", "Runs a database command, e.g. { ping: 1 }."),
  method("adminCommand", "(command)", "Runs a command on the admin database."),
  method("createCollection", "(name, options?)", "Creates a collection."),
  method("createView", "(name, source, pipeline)", "Creates a view."),
  method("dropDatabase", "()", "Drops the current database."),
  method("stats", "()", "The database's storage statistics."),
  method("version", "()", "The server's build information."),
];

const COLLECTION_METHODS: Completion[] = [
  method("find", "(filter?, projection?)", "Documents matching the filter."),
  method("findOne", "(filter?, projection?)", "The first document matching the filter."),
  method("aggregate", "([stages])", "Runs an aggregation pipeline."),
  method("countDocuments", "(filter?)", "How many documents match."),
  method("estimatedDocumentCount", "()", "The document count from the metadata."),
  method("distinct", "(field, filter?)", "The distinct values of a field."),
  method("insertOne", "(document)", "Inserts a document."),
  method("insertMany", "([documents])", "Inserts documents."),
  method("updateOne", "(filter, update, options?)", "Updates the first matching document."),
  method("updateMany", "(filter, update, options?)", "Updates every matching document."),
  method("replaceOne", "(filter, document, options?)", "Replaces the first matching document."),
  method("deleteOne", "(filter)", "Deletes the first matching document."),
  method("deleteMany", "(filter)", "Deletes every matching document."),
  method("createIndex", "(keys, options?)", "Creates an index."),
  method("dropIndex", "(name)", "Drops an index."),
  method("getIndexes", "()", "The collection's indexes."),
  method("renameCollection", "(name)", "Renames the collection."),
  method("drop", "()", "Drops the collection."),
];

const CURSOR_METHODS: Completion[] = [
  method("sort", "({ field: 1 })", "Orders the documents; -1 for descending."),
  method("limit", "(n)", "At most n documents."),
  method("skip", "(n)", "Skips the first n documents."),
  method("projection", "({ field: 1 })", "Only these fields."),
  method("count", "()", "How many documents the query matches."),
  method("toArray", "()", "All the documents (the grid reads them anyway)."),
];

const OPERATORS = [
  "$eq", "$ne", "$gt", "$gte", "$lt", "$lte", "$in", "$nin", "$and", "$or", "$nor", "$not",
  "$exists", "$type", "$regex", "$elemMatch", "$size", "$all", "$expr",
  "$set", "$unset", "$inc", "$push", "$pull", "$addToSet", "$rename",
  "$match", "$group", "$project", "$sort", "$limit", "$skip", "$unwind", "$lookup", "$count",
  "$addFields", "$sum", "$avg", "$min", "$max", "$first", "$last",
].map((label): Completion => ({ label, type: "keyword" }));

const STATEMENTS: Completion[] = [
  { label: "show dbs", type: "keyword", info: "The server's databases." },
  { label: "show collections", type: "keyword", info: "The collections of the current database." },
  { label: "use", type: "keyword", info: "Switches to another database.", apply: "use " },
  { label: "db", type: "variable", info: "The current database." },
];

/** The collections of the default database, with their sampled fields. */
function collections(catalog: CompletionCatalog | null | undefined): Record<string, string[]> {
  if (!catalog?.default_schema) return {};
  return catalog.schemas[catalog.default_schema] ?? {};
}

/** The collection named at the start of `text`, a statement: `db.name.` or
 *  `db.getCollection("name").`. */
function statementCollection(text: string): string | null {
  const m = /db\s*\.\s*(?:getCollection\(\s*["'`]([^"'`]+)["'`]\s*\)|([A-Za-z_$][\w$]*))\s*\.\s*[A-Za-z]+\s*\(/.exec(text);
  return m ? (m[1] ?? m[2] ?? null) : null;
}

/** What completions stay valid for while the user types a name. */
const IDENT = /^[\w$]*$/;

function completeMongo(catalog: CompletionCatalog | null | undefined) {
  return (context: CompletionContext): CompletionResult | null => {
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    const word = context.matchBefore(/[\w$]*/);
    const from = word?.from ?? context.pos;
    const colls = collections(catalog);

    // `db.` → collections and database methods.
    if (/(^|[^\w$.])db\s*\.\s*[\w$]*$/.test(before)) {
      const names = Object.keys(colls).map((name): Completion => ({ label: name, type: "class", boost: 1 }));
      return { from, options: [...names, ...DB_METHODS], validFor: IDENT };
    }
    // `db.name.` → collection methods; `….find(…).` → cursor methods.
    if (/db\s*\.\s*(?:getCollection\([^)]*\)|[\w$]+)\s*\.\s*[\w$]*$/.test(before)) {
      return { from, options: COLLECTION_METHODS, validFor: IDENT };
    }
    if (/\)\s*\.\s*[\w$]*$/.test(before)) {
      return { from, options: CURSOR_METHODS, validFor: IDENT };
    }
    // A `$` operator, or a field of the statement's collection where a key goes.
    if (/[{,]\s*["']?\$?[\w$]*$/.test(before)) {
      const dollar = context.matchBefore(/\$[\w$]*/);
      if (dollar) return { from: dollar.from, options: OPERATORS, validFor: /^\$[\w$]*$/ };
      if (!word || (word.from === word.to && !context.explicit)) return null;
      const start = context.state.doc.sliceString(Math.max(0, context.pos - 2000), context.pos);
      const coll = statementCollection(start.slice(Math.max(0, start.lastIndexOf("db."))));
      const fields = (coll ? colls[coll] : undefined) ?? [];
      return {
        from,
        options: fields.map((f): Completion => ({ label: f, type: "property" })),
        validFor: IDENT,
      };
    }
    // The start of a statement.
    if (/^\s*[\w ]*$/.test(before) && (word?.from !== word?.to || context.explicit)) {
      return { from: line.from + before.length - before.trimStart().length, options: STATEMENTS };
    }
    return null;
  };
}

/** mongosh highlighting and completion for a MongoDB editor. */
export function mongoLanguage(catalog: CompletionCatalog | null | undefined) {
  const support = javascript();
  return [support, support.language.data.of({ autocomplete: completeMongo(catalog) })];
}

/** A mongosh string literal. */
export const shellString = (s: string) => JSON.stringify(s);

/** A collection in mongosh: `db.name`, or `db.getCollection("name")` for other names, with
 *  `getSiblingDB` first when `schema` is not the session's database. Mirrors
 *  `generate::collection_ref` in `platypus-driver-mongo`. */
export function collectionRef(schema: string | null, name: string): string {
  const base = schema === null ? "db" : `db.getSiblingDB(${shellString(schema)})`;
  return /^[A-Za-z_$][\w$]*$/.test(name) && !name.startsWith("get")
    ? `${base}.${name}`
    : `${base}.getCollection(${shellString(name)})`;
}
