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
  | { kind: "bool" | "first_name" | "last_name" | "full_name" | "email" | "city" | "phone" | "text" | "date" | "timestamp" | "uuid" | "null" }
  | { kind: "object_id" | "word_list" | "document" };

export type Kind = Generator["kind"];

/** Generators of MongoDB's types, offered for its collections only. */
export const MONGO_ONLY: Kind[] = ["object_id", "word_list", "document"];

/** Generators that repeat values, which a unique column refuses. */
export const REPEATING: Kind[] = ["integer", "decimal", "bool", "choice", "date", "timestamp"];

/** The generator a column most likely wants, from its keys, name and type; `mongo` for a
 * field of a MongoDB collection, typed as the documents sampled. */
export function suggest(column: ColumnMeta, fk: ForeignKeyMeta | undefined, unique = false, mongo = false): Kind {
  const name = column.name.toLowerCase();
  const type = column.data_type.toLowerCase();
  const integer = /int|serial|number/.test(type) || (mongo && type === "long");
  if (fk) return "reference";
  if (column.default !== null) return "skip";
  if (mongo && type === "objectid") return "object_id";
  if (mongo && type === "array") return "word_list";
  if (mongo && type === "document") return "document";
  if (unique && (integer || /numeric|decimal/.test(type))) return "next_key";
  if (column.primary_key) return integer ? "next_key" : /uuid|uniqueidentifier/.test(type) ? "uuid" : "next_key";
  if (column.enum_values.length > 0) return "choice";
  if (name.includes("email")) return "email";
  if (/first.?name/.test(name)) return "first_name";
  if (/last.?name|surname/.test(name)) return "last_name";
  if (name.includes("name")) return "full_name";
  if (name.includes("city")) return "city";
  if (/phone|tel/.test(name)) return "phone";
  if (/bool|^bit/.test(type)) return "bool";
  if (/timestamp|datetime/.test(type)) return "timestamp";
  if (/date/.test(type)) return "date";
  if (/uuid|uniqueidentifier/.test(type)) return "uuid";
  if (/numeric|decimal|real|float|double|money/.test(type)) return "decimal";
  if (integer) return "integer";
  return "text";
}
