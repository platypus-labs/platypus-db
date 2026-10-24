/**
 * Built-in functions for autocompletion, with their signature and a line of documentation.
 * The letters say which dialects have each one: p PostgreSQL, m MySQL, s SQLite.
 */
import { type Completion, type CompletionContext, type CompletionSource, snippetCompletion } from "@codemirror/autocomplete";
import type { DriverKind } from "../api";

type Entry = [name: string, args: string, doc: string, dialects: string];

const FUNCTIONS: Entry[] = [
  // Aggregates
  ["count", "expr | *", "Number of rows, or of non-NULL values of expr.", "pms"],
  ["sum", "expr", "Sum of the non-NULL values.", "pms"],
  ["avg", "expr", "Average of the non-NULL values.", "pms"],
  ["min", "expr", "Smallest value.", "pms"],
  ["max", "expr", "Largest value.", "pms"],
  ["string_agg", "expr, delimiter [ORDER BY …]", "Values joined into one string with delimiter.", "p"],
  ["array_agg", "expr [ORDER BY …]", "Values collected into an array.", "p"],
  ["json_agg", "expr", "Values collected into a JSON array.", "p"],
  ["jsonb_agg", "expr", "Values collected into a jsonb array.", "p"],
  ["bool_and", "expr", "True if every value is true.", "p"],
  ["bool_or", "expr", "True if any value is true.", "p"],
  ["group_concat", "expr [ORDER BY …] [SEPARATOR sep]", "Values joined into one string, by default with commas.", "ms"],
  ["json_arrayagg", "expr", "Values collected into a JSON array.", "m"],
  ["json_group_array", "expr", "Values collected into a JSON array.", "s"],
  ["total", "expr", "Sum as a float; 0.0 when there are no rows.", "s"],
  // Window functions
  ["row_number", "", "Number of the row in its partition, from 1. Needs OVER (…).", "pms"],
  ["rank", "", "Rank with gaps for ties. Needs OVER (…).", "pms"],
  ["dense_rank", "", "Rank without gaps for ties. Needs OVER (…).", "pms"],
  ["lag", "expr [, offset [, default]]", "Value from a row before the current one. Needs OVER (…).", "pms"],
  ["lead", "expr [, offset [, default]]", "Value from a row after the current one. Needs OVER (…).", "pms"],
  ["first_value", "expr", "Value at the first row of the window frame.", "pms"],
  ["last_value", "expr", "Value at the last row of the window frame.", "pms"],
  ["ntile", "buckets", "Divides the partition into buckets numbered from 1.", "pms"],
  // Conditional
  ["coalesce", "value, value, …", "The first value that is not NULL.", "pms"],
  ["nullif", "a, b", "NULL when a = b, otherwise a.", "pms"],
  ["greatest", "value, value, …", "The largest of the values.", "pm"],
  ["least", "value, value, …", "The smallest of the values.", "pm"],
  ["ifnull", "expr, fallback", "fallback when expr is NULL.", "ms"],
  ["if", "condition, then, else", "then when condition is true, else otherwise.", "m"],
  ["iif", "condition, then, else", "then when condition is true, else otherwise.", "s"],
  // Strings
  ["length", "text", "Length in characters (bytes for MySQL; use char_length).", "pms"],
  ["char_length", "text", "Length in characters.", "pm"],
  ["lower", "text", "Text in lower case.", "pms"],
  ["upper", "text", "Text in upper case.", "pms"],
  ["trim", "text", "Text without leading and trailing spaces.", "pms"],
  ["ltrim", "text", "Text without leading spaces.", "pms"],
  ["rtrim", "text", "Text without trailing spaces.", "pms"],
  ["substring", "text FROM start [FOR count] | text, start [, count]", "Part of the text, from 1.", "pm"],
  ["substr", "text, start [, count]", "Part of the text, from 1.", "pms"],
  ["replace", "text, from, to", "Text with every from replaced by to.", "pms"],
  ["concat", "value, value, …", "Values joined into one string; NULLs are skipped (MySQL: NULL if any is NULL).", "pm"],
  ["concat_ws", "separator, value, …", "Values joined with separator, skipping NULLs.", "pm"],
  ["left", "text, n", "First n characters.", "pm"],
  ["right", "text, n", "Last n characters.", "pm"],
  ["lpad", "text, length [, fill]", "Text padded on the left to length.", "pm"],
  ["rpad", "text, length [, fill]", "Text padded on the right to length.", "pm"],
  ["position", "sub IN text", "Where sub starts in text, from 1; 0 when absent.", "pm"],
  ["instr", "text, sub", "Where sub starts in text, from 1; 0 when absent.", "ms"],
  ["split_part", "text, delimiter, n", "The n-th field of text split by delimiter.", "p"],
  ["regexp_replace", "text, pattern, replacement [, flags]", "Text with regular expression matches replaced.", "pm"],
  ["regexp_matches", "text, pattern [, flags]", "The captured groups of each match.", "p"],
  ["format", "format, value, …", "Text formatted printf-style (%s, %I, %L).", "p"],
  ["printf", "format, value, …", "Text formatted printf-style.", "s"],
  ["md5", "text", "MD5 hash as hex text.", "pm"],
  ["to_char", "value, format", "Date or number as text, e.g. 'YYYY-MM-DD'.", "p"],
  // Numbers
  ["abs", "x", "Absolute value.", "pms"],
  ["round", "x [, digits]", "x rounded to digits decimals.", "pms"],
  ["ceil", "x", "Smallest integer not less than x.", "pm"],
  ["floor", "x", "Largest integer not greater than x.", "pm"],
  ["mod", "a, b", "Remainder of a / b.", "pm"],
  ["power", "a, b", "a raised to b.", "pm"],
  ["sqrt", "x", "Square root.", "pm"],
  ["random", "", "Random number: 0 ≤ x < 1 (SQLite: a random 64-bit integer).", "ps"],
  ["rand", "[seed]", "Random number: 0 ≤ x < 1.", "m"],
  // Dates
  ["now", "", "Current date and time (start of the transaction in PostgreSQL).", "pm"],
  ["date_trunc", "unit, timestamp", "Timestamp truncated to 'day', 'month', 'year'…", "p"],
  ["date_part", "unit, timestamp", "A field of the timestamp, e.g. 'year'.", "p"],
  ["age", "timestamp [, timestamp]", "Interval between the timestamps, or since it.", "p"],
  ["make_date", "year, month, day", "A date from its parts.", "p"],
  ["to_timestamp", "text, format | epoch", "Timestamp from text in format, or from Unix seconds.", "p"],
  ["date_format", "date, format", "Date as text, e.g. '%Y-%m-%d'.", "m"],
  ["date_add", "date, INTERVAL n unit", "Date plus an interval.", "m"],
  ["datediff", "a, b", "Days from b to a.", "m"],
  ["timestampdiff", "unit, a, b", "Difference b - a in unit.", "m"],
  ["curdate", "", "Current date.", "m"],
  ["date", "time-value [, modifier, …]", "Date as YYYY-MM-DD.", "ms"],
  ["datetime", "time-value [, modifier, …]", "Date and time as YYYY-MM-DD HH:MM:SS.", "s"],
  ["strftime", "format, time-value [, modifier, …]", "Date and time as text, e.g. '%Y-%m'.", "s"],
  ["julianday", "time-value [, modifier, …]", "Julian day number.", "s"],
  ["unixepoch", "time-value [, modifier, …]", "Unix seconds.", "s"],
  // JSON
  ["jsonb_build_object", "key, value, …", "A jsonb object from key-value pairs.", "p"],
  ["json_build_object", "key, value, …", "A JSON object from key-value pairs.", "p"],
  ["jsonb_array_elements", "jsonb", "A row per element of the array.", "p"],
  ["jsonb_extract_path_text", "jsonb, key, …", "The value at the path, as text.", "p"],
  ["json_object", "key, value, …", "A JSON object from key-value pairs.", "ms"],
  ["json_extract", "json, path, …", "The value at a path such as '$.a.b'.", "ms"],
  ["json_unquote", "json", "A JSON string value without its quotes.", "m"],
  // Sets, types and system
  ["generate_series", "start, stop [, step]", "A row per value from start to stop.", "p"],
  ["unnest", "array", "A row per element of the array.", "p"],
  ["array_length", "array, dimension", "Length of the array in that dimension.", "p"],
  ["gen_random_uuid", "", "A random version 4 UUID.", "p"],
  ["uuid", "", "A version 1 UUID.", "m"],
  ["current_setting", "name", "Value of a server setting.", "p"],
  ["pg_size_pretty", "bytes", "Size in kB, MB, GB…", "p"],
  ["pg_total_relation_size", "regclass", "Bytes of a table with its indexes and TOAST.", "p"],
  ["typeof", "expr", "Storage class of the value: null, integer, real, text or blob.", "s"],
  ["last_insert_id", "", "The AUTO_INCREMENT value of the last insert.", "m"],
  ["last_insert_rowid", "", "The rowid of the last insert.", "s"],
  ["version", "", "Server version.", "pm"],
];

const LETTER: Record<DriverKind, string> = { postgres: "p", mysql: "m", sqlite: "s" };

function options(driver: DriverKind): Completion[] {
  return FUNCTIONS.filter((f) => f.at(3)!.includes(LETTER[driver])).map(([name, args, doc]) =>
    snippetCompletion(args ? `${name}(\${})` : `${name}()`, {
      label: name,
      type: "function",
      detail: `(${args})`,
      info: doc,
      boost: -1,
    }),
  );
}

const cache = new Map<DriverKind, Completion[]>();

/** Completes the dialect's built-in functions after two letters of a word. */
export function functionCompletion(driver: DriverKind): CompletionSource {
  return (context: CompletionContext) => {
    const word = context.matchBefore(/\w+/);
    if (!word || (word.to - word.from < 2 && !context.explicit)) return null;
    // Not after a dot: `o.co` completes columns, not functions.
    if (context.state.sliceDoc(word.from - 1, word.from) === ".") return null;
    if (!cache.has(driver)) cache.set(driver, options(driver));
    return { from: word.from, options: cache.get(driver)!, validFor: /^\w*$/ };
  };
}
