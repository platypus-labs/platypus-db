/** The generators of fake rows, and the one a column most likely wants. */
import type { ColumnMeta, ForeignKeyMeta } from "../api";

/** How a column is filled, as the backend's `fake::Generator`; `skip` leaves it to its default. */
export type Generator =
  | { kind: "skip" }
  | { kind: "integer"; min: number; max: number }
  | { kind: "decimal"; min: number; max: number; scale: number }
  | { kind: "next_key" }
  | { kind: "reference"; schema: string | null; table: string; column: string; key: string }
  | { kind: "choice"; values: string[] }
  | { kind: "bool" | "first_name" | "last_name" | "full_name" | "email" | "city" | "phone" | "text" | "date" | "timestamp" | "time" | "ip" | "uuid" | "null" }
  | { kind: "object_id" | "word_list" | "document" };

export type Kind = Generator["kind"];

/** Generators of MongoDB's types, offered for its collections only. */
export const MONGO_ONLY: Kind[] = ["object_id", "word_list", "document"];

/** Generators of JSON values, offered for a JSON column too. */
export const JSON_KINDS: Kind[] = ["word_list", "document"];

/** Generators that repeat values, which a unique column refuses. */
export const REPEATING: Kind[] = ["integer", "decimal", "bool", "choice", "date", "timestamp", "time", "ip", "word_list", "document"];

/** Whether a column of type `type` (lower case) holds JSON. */
export const isJson = (type: string) => /^jsonb?$/.test(type);

/** Whether a column has a default of its own: MariaDB writes no default as the text `NULL`. */
const hasDefault = (column: ColumnMeta) => column.default !== null && !/^null$/i.test(column.default.trim());

/** Whether a type holds text, which the name, city and word generators fill (Oracle's
 *  `LONG` is text). */
const isText = (type: string) => /char|text|string|clob|citext|^name$|^long$/.test(type);

/** Whether a type holds several values: arrays (`text[]`, `array(integer)`), maps, tuples,
 *  sets, and DuckDB's STRUCT and UNION or Trino's ROW, whose field types may say `int`. */
const isComposite = (type: string) =>
  /\[\]$|^_|^(array|map|tuple|nested|struct|union|row)\b|^set\(/.test(type);

/** Whether a type holds whole numbers: `interval` and `point` hold no `int`. */
const isInteger = (type: string) =>
  /int|serial|number|^year/.test(type) && !/interval|point/.test(type) && !isComposite(type);

/** The generator of a date or time type, `null` for another type. */
const temporal = (type: string): Kind | null => {
  if (/timestamp|datetime/.test(type)) return "timestamp";
  if (/^date/.test(type)) return "date";
  if (/^time\b|^time$|^time\(/.test(type)) return "time";
  return null;
};

/** The generator a column most likely wants, from its keys, name and type; `mongo` for a
 * field of a MongoDB collection, typed as the documents sampled; `generated` when the
 * database fills it by itself (an identity, a computed column). A type no generator fits is
 * left NULL, or to its default when it takes no NULL. */
export function suggest(column: ColumnMeta, fk: ForeignKeyMeta | undefined, unique = false, mongo = false, generated = false): Kind {
  const name = column.name.toLowerCase();
  const type = column.data_type.toLowerCase().trim();
  const integer = isInteger(type) || (mongo && type === "long");
  if (generated) return "skip";
  if (fk) return "reference";
  if (hasDefault(column)) return "skip";
  if (mongo && type === "objectid") return "object_id";
  if (mongo && type === "array") return "word_list";
  if (mongo && type === "document") return "document";
  // An `_id` of text or of several types: numbers would land among other keys.
  if (mongo && column.primary_key && !integer && !/double|decimal/.test(type)) return "object_id";
  if (unique && (integer || /numeric|decimal/.test(type))) return "next_key";
  if (column.primary_key) {
    if (integer) return "next_key";
    if (/uuid|uniqueidentifier/.test(type)) return "uuid";
    // A key of several columns with a time in it (a hypertable's device and time) takes
    // times: numbers do not go into a date column.
    const time = unique ? null : temporal(type);
    return time ?? "next_key";
  }
  if (column.enum_values.length > 0) return "choice";
  // Arrays (`text[]`, ClickHouse's `Array(String)`), maps, tuples, sets, structs, unions and
  // rows: no generator.
  if (isComposite(type)) return column.nullable ? "null" : "skip";
  // MySQL's BOOLEAN is a tinyint(1); a longer bit string is no flag.
  if (/bool|^bit$|^bit\(1\)$|^tinyint\(1\)$/.test(type)) return "bool";
  if ((isText(type) && !integer) || (mongo && type === "string")) {
    if (name.includes("email")) return "email";
    if (/first.?name/.test(name)) return "first_name";
    if (/last.?name|surname/.test(name)) return "last_name";
    if (name.includes("name")) return "full_name";
    if (name.includes("city")) return "city";
    if (/phone|tel/.test(name)) return "phone";
    return "text";
  }
  if (isJson(type)) return "document";
  // BSON's date holds the time too.
  if (mongo && type === "date") return "timestamp";
  const time = temporal(type);
  if (time) return time;
  if (/uuid|uniqueidentifier/.test(type)) return "uuid";
  if (/^(inet|cidr)/.test(type)) return "ip";
  if (/^(numeric|decimal|real|float|double|money|smallmoney|number)/.test(type) && !integer) return "decimal";
  if (integer) return "integer";
  if (mongo) return "text";
  return column.nullable ? "null" : "skip";
}

/** The bounds of the numbers that fit a column of `type`: a small integer, a year, a
 *  decimal of a few digits before its point. */
export function numberRange(type: string): { min: number; max: number; scale: number } {
  const t = type.toLowerCase();
  if (/^year/.test(t)) return { min: 1990, max: 2030, scale: 0 };
  if (/tinyint|^u?int8$|^int1$/.test(t)) return { min: 0, max: 100, scale: 0 };
  const decimal = /\((\d+)\s*(?:,\s*(\d+))?\)/.exec(t);
  if (decimal && /numeric|decimal|number/.test(t)) {
    const precision = Number(decimal[1]);
    const scale = Number(decimal[2] ?? 0);
    const max = Math.min(1000, 10 ** (precision - scale) - 1);
    return { min: 0, max: Math.max(max, 0), scale: Math.min(scale, 2) };
  }
  return { min: t.includes("int") || t === "year" ? 1 : 0, max: 1000, scale: 2 };
}
