/**
 * Built-in functions for autocompletion, with their signature and a line of documentation.
 * The letters say which dialects have each one: p PostgreSQL, m MySQL, s SQLite, d DuckDB.
 */
import { type Completion, type CompletionContext, type CompletionSource, snippetCompletion } from "@codemirror/autocomplete";
import type { DriverKind } from "../api";

type Entry = [name: string, args: string, doc: string, dialects: string];

const FUNCTIONS: Entry[] = [
  // Aggregates
  ["count", "expr | *", "Number of rows, or of non-NULL values of expr.", "pmsd"],
  ["sum", "expr", "Sum of the non-NULL values.", "pmsd"],
  ["avg", "expr", "Average of the non-NULL values.", "pmsd"],
  ["min", "expr", "Smallest value.", "pmsd"],
  ["max", "expr", "Largest value.", "pmsd"],
  ["string_agg", "expr, delimiter [ORDER BY …]", "Values joined into one string with delimiter.", "pd"],
  ["array_agg", "expr [ORDER BY …]", "Values collected into an array.", "pd"],
  ["json_agg", "expr", "Values collected into a JSON array.", "p"],
  ["jsonb_agg", "expr", "Values collected into a jsonb array.", "p"],
  ["bool_and", "expr", "True if every value is true.", "pd"],
  ["bool_or", "expr", "True if any value is true.", "pd"],
  ["group_concat", "expr [ORDER BY …] [SEPARATOR sep]", "Values joined into one string, by default with commas.", "ms"],
  ["json_arrayagg", "expr", "Values collected into a JSON array.", "m"],
  ["json_group_array", "expr", "Values collected into a JSON array.", "sd"],
  ["total", "expr", "Sum as a float; 0.0 when there are no rows.", "s"],
  // Window functions
  ["row_number", "", "Number of the row in its partition, from 1. Needs OVER (…).", "pmsd"],
  ["rank", "", "Rank with gaps for ties. Needs OVER (…).", "pmsd"],
  ["dense_rank", "", "Rank without gaps for ties. Needs OVER (…).", "pmsd"],
  ["lag", "expr [, offset [, default]]", "Value from a row before the current one. Needs OVER (…).", "pmsd"],
  ["lead", "expr [, offset [, default]]", "Value from a row after the current one. Needs OVER (…).", "pmsd"],
  ["first_value", "expr", "Value at the first row of the window frame.", "pmsd"],
  ["last_value", "expr", "Value at the last row of the window frame.", "pmsd"],
  ["ntile", "buckets", "Divides the partition into buckets numbered from 1.", "pmsd"],
  // Conditional
  ["coalesce", "value, value, …", "The first value that is not NULL.", "pmsd"],
  ["nullif", "a, b", "NULL when a = b, otherwise a.", "pmsd"],
  ["greatest", "value, value, …", "The largest of the values.", "pmd"],
  ["least", "value, value, …", "The smallest of the values.", "pmd"],
  ["ifnull", "expr, fallback", "fallback when expr is NULL.", "msd"],
  ["if", "condition, then, else", "then when condition is true, else otherwise.", "md"],
  ["iif", "condition, then, else", "then when condition is true, else otherwise.", "s"],
  // Strings
  ["length", "text", "Length in characters (bytes for MySQL; use char_length).", "pmsd"],
  ["char_length", "text", "Length in characters.", "pmd"],
  ["lower", "text", "Text in lower case.", "pmsd"],
  ["upper", "text", "Text in upper case.", "pmsd"],
  ["trim", "text", "Text without leading and trailing spaces.", "pmsd"],
  ["ltrim", "text", "Text without leading spaces.", "pmsd"],
  ["rtrim", "text", "Text without trailing spaces.", "pmsd"],
  ["substring", "text FROM start [FOR count] | text, start [, count]", "Part of the text, from 1.", "pmd"],
  ["substr", "text, start [, count]", "Part of the text, from 1.", "pmsd"],
  ["replace", "text, from, to", "Text with every from replaced by to.", "pmsd"],
  ["concat", "value, value, …", "Values joined into one string; NULLs are skipped (MySQL: NULL if any is NULL).", "pmd"],
  ["concat_ws", "separator, value, …", "Values joined with separator, skipping NULLs.", "pmd"],
  ["left", "text, n", "First n characters.", "pmd"],
  ["right", "text, n", "Last n characters.", "pmd"],
  ["lpad", "text, length [, fill]", "Text padded on the left to length.", "pmd"],
  ["rpad", "text, length [, fill]", "Text padded on the right to length.", "pmd"],
  ["position", "sub IN text", "Where sub starts in text, from 1; 0 when absent.", "pmd"],
  ["instr", "text, sub", "Where sub starts in text, from 1; 0 when absent.", "msd"],
  ["split_part", "text, delimiter, n", "The n-th field of text split by delimiter.", "pd"],
  ["regexp_replace", "text, pattern, replacement [, flags]", "Text with regular expression matches replaced.", "pmd"],
  ["regexp_matches", "text, pattern [, flags]", "The captured groups of each match.", "p"],
  ["format", "format, value, …", "Text formatted printf-style (%s, %I, %L).", "p"],
  ["printf", "format, value, …", "Text formatted printf-style.", "sd"],
  ["md5", "text", "MD5 hash as hex text.", "pmd"],
  ["to_char", "value, format", "Date or number as text, e.g. 'YYYY-MM-DD'.", "p"],
  // Numbers
  ["abs", "x", "Absolute value.", "pmsd"],
  ["round", "x [, digits]", "x rounded to digits decimals.", "pmsd"],
  ["ceil", "x", "Smallest integer not less than x.", "pmd"],
  ["floor", "x", "Largest integer not greater than x.", "pmd"],
  ["mod", "a, b", "Remainder of a / b.", "pmd"],
  ["power", "a, b", "a raised to b.", "pmd"],
  ["sqrt", "x", "Square root.", "pmd"],
  ["random", "", "Random number: 0 ≤ x < 1 (SQLite: a random 64-bit integer).", "psd"],
  ["rand", "[seed]", "Random number: 0 ≤ x < 1.", "m"],
  // Dates
  ["now", "", "Current date and time (start of the transaction in PostgreSQL).", "pmd"],
  ["date_trunc", "unit, timestamp", "Timestamp truncated to 'day', 'month', 'year'…", "pd"],
  ["date_part", "unit, timestamp", "A field of the timestamp, e.g. 'year'.", "pd"],
  ["age", "timestamp [, timestamp]", "Interval between the timestamps, or since it.", "pd"],
  ["make_date", "year, month, day", "A date from its parts.", "pd"],
  ["to_timestamp", "text, format | epoch", "Timestamp from text in format, or from Unix seconds.", "p"],
  ["date_format", "date, format", "Date as text, e.g. '%Y-%m-%d'.", "m"],
  ["date_add", "date, INTERVAL n unit", "Date plus an interval.", "md"],
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
  ["json_object", "key, value, …", "A JSON object from key-value pairs.", "msd"],
  ["json_extract", "json, path, …", "The value at a path such as '$.a.b'.", "msd"],
  ["json_unquote", "json", "A JSON string value without its quotes.", "m"],
  // Sets, types and system
  ["generate_series", "start, stop [, step]", "A row per value from start to stop.", "pd"],
  ["unnest", "array", "A row per element of the array.", "pd"],
  ["array_length", "array, dimension", "Length of the array in that dimension.", "pd"],
  ["gen_random_uuid", "", "A random version 4 UUID.", "pd"],
  ["uuid", "", "A version 1 UUID.", "m"],
  ["current_setting", "name", "Value of a server setting.", "pd"],
  ["pg_size_pretty", "bytes", "Size in kB, MB, GB…", "pd"],
  ["pg_total_relation_size", "regclass", "Bytes of a table with its indexes and TOAST.", "p"],
  ["typeof", "expr", "Storage class of the value: null, integer, real, text or blob.", "s"],
  ["last_insert_id", "", "The AUTO_INCREMENT value of the last insert.", "m"],
  ["last_insert_rowid", "", "The rowid of the last insert.", "s"],
  ["version", "", "Server version.", "pmd"],
];

const LETTER: Record<DriverKind, string> = { postgres: "p", mysql: "m", sqlite: "s", duckdb: "d" };

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
