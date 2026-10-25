/**
 * Built-in functions for autocompletion, with their signature and a line of documentation.
 * The letters say which dialects have each one: p PostgreSQL, m MySQL, s SQLite, d DuckDB,
 * t SQL Server (Transact-SQL), c ClickHouse.
 */
import { type Completion, type CompletionContext, type CompletionSource, snippetCompletion } from "@codemirror/autocomplete";
import type { DriverKind } from "../api";

type Entry = [name: string, args: string, doc: string, dialects: string];

const FUNCTIONS: Entry[] = [
  // Aggregates
  ["count", "expr | *", "Number of rows, or of non-NULL values of expr.", "pmsdtc"],
  ["sum", "expr", "Sum of the non-NULL values.", "pmsdtc"],
  ["avg", "expr", "Average of the non-NULL values.", "pmsdtc"],
  ["min", "expr", "Smallest value.", "pmsdtc"],
  ["max", "expr", "Largest value.", "pmsdtc"],
  ["string_agg", "expr, delimiter [ORDER BY …]", "Values joined into one string with delimiter.", "pdt"],
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
  ["row_number", "", "Number of the row in its partition, from 1. Needs OVER (…).", "pmsdtc"],
  ["rank", "", "Rank with gaps for ties. Needs OVER (…).", "pmsdtc"],
  ["dense_rank", "", "Rank without gaps for ties. Needs OVER (…).", "pmsdtc"],
  ["lag", "expr [, offset [, default]]", "Value from a row before the current one. Needs OVER (…).", "pmsdt"],
  ["lead", "expr [, offset [, default]]", "Value from a row after the current one. Needs OVER (…).", "pmsdt"],
  ["first_value", "expr", "Value at the first row of the window frame.", "pmsdtc"],
  ["last_value", "expr", "Value at the last row of the window frame.", "pmsdtc"],
  ["ntile", "buckets", "Divides the partition into buckets numbered from 1.", "pmsdtc"],
  // Conditional
  ["coalesce", "value, value, …", "The first value that is not NULL.", "pmsdtc"],
  ["nullif", "a, b", "NULL when a = b, otherwise a.", "pmsdtc"],
  ["greatest", "value, value, …", "The largest of the values.", "pmdtc"],
  ["least", "value, value, …", "The smallest of the values.", "pmdtc"],
  ["ifnull", "expr, fallback", "fallback when expr is NULL.", "msdc"],
  ["if", "condition, then, else", "then when condition is true, else otherwise.", "mdc"],
  ["iif", "condition, then, else", "then when condition is true, else otherwise.", "st"],
  // Strings
  ["length", "text", "Length in characters (bytes for MySQL; use char_length).", "pmsdc"],
  ["char_length", "text", "Length in characters.", "pmdc"],
  ["lower", "text", "Text in lower case.", "pmsdtc"],
  ["upper", "text", "Text in upper case.", "pmsdtc"],
  ["trim", "text", "Text without leading and trailing spaces.", "pmsdtc"],
  ["ltrim", "text", "Text without leading spaces.", "pmsdtc"],
  ["rtrim", "text", "Text without trailing spaces.", "pmsdtc"],
  ["substring", "text FROM start [FOR count] | text, start [, count]", "Part of the text, from 1.", "pmdc"],
  ["substr", "text, start [, count]", "Part of the text, from 1.", "pmsdc"],
  ["replace", "text, from, to", "Text with every from replaced by to.", "pmsdtc"],
  ["concat", "value, value, …", "Values joined into one string; NULLs are skipped (MySQL: NULL if any is NULL).", "pmdtc"],
  ["concat_ws", "separator, value, …", "Values joined with separator, skipping NULLs.", "pmdtc"],
  ["left", "text, n", "First n characters.", "pmdtc"],
  ["right", "text, n", "Last n characters.", "pmdtc"],
  ["lpad", "text, length [, fill]", "Text padded on the left to length.", "pmdc"],
  ["rpad", "text, length [, fill]", "Text padded on the right to length.", "pmdc"],
  ["position", "sub IN text", "Where sub starts in text, from 1; 0 when absent.", "pmdc"],
  ["instr", "text, sub", "Where sub starts in text, from 1; 0 when absent.", "msd"],
  ["split_part", "text, delimiter, n", "The n-th field of text split by delimiter.", "pd"],
  ["regexp_replace", "text, pattern, replacement [, flags]", "Text with regular expression matches replaced.", "pmd"],
  ["regexp_matches", "text, pattern [, flags]", "The captured groups of each match.", "p"],
  ["format", "format, value, …", "Text formatted printf-style (%s, %I, %L).", "p"],
  ["printf", "format, value, …", "Text formatted printf-style.", "sd"],
  ["md5", "text", "MD5 hash as hex text.", "pmdc"],
  ["to_char", "value, format", "Date or number as text, e.g. 'YYYY-MM-DD'.", "p"],
  // Numbers
  ["abs", "x", "Absolute value.", "pmsdtc"],
  ["round", "x [, digits]", "x rounded to digits decimals.", "pmsdtc"],
  ["ceil", "x", "Smallest integer not less than x.", "pmdc"],
  ["floor", "x", "Largest integer not greater than x.", "pmdtc"],
  ["mod", "a, b", "Remainder of a / b.", "pmdc"],
  ["power", "a, b", "a raised to b.", "pmdtc"],
  ["sqrt", "x", "Square root.", "pmdtc"],
  ["random", "", "Random number: 0 ≤ x < 1 (SQLite: a random 64-bit integer).", "psd"],
  ["rand", "[seed]", "Random number: 0 ≤ x < 1.", "mtc"],
  // Dates
  ["now", "", "Current date and time (start of the transaction in PostgreSQL).", "pmdc"],
  ["date_trunc", "unit, timestamp", "Timestamp truncated to 'day', 'month', 'year'…", "pdc"],
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
  ["version", "", "Server version.", "pmdc"],
  // Transact-SQL
  ["len", "text", "Length in characters, without trailing spaces.", "t"],
  ["datalength", "expr", "Length in bytes.", "t"],
  ["charindex", "sub, text [, start]", "Where sub starts in text, from 1; 0 when absent.", "t"],
  ["substring", "text, start, count", "Part of the text, from 1.", "t"],
  ["stuff", "text, start, count, insert", "Text with count characters from start replaced by insert.", "t"],
  ["format", "value, format [, culture]", "Date or number as text in a .NET format, e.g. 'yyyy-MM-dd'.", "t"],
  ["isnull", "expr, fallback", "fallback when expr is NULL.", "t"],
  ["ceiling", "x", "Smallest integer not less than x.", "t"],
  ["getdate", "", "Current date and time as datetime.", "t"],
  ["sysdatetime", "", "Current date and time as datetime2(7).", "t"],
  ["sysdatetimeoffset", "", "Current date and time with the server's offset.", "t"],
  ["dateadd", "unit, n, date", "Date plus n units, e.g. dateadd(day, 1, d).", "t"],
  ["datediff", "unit, a, b", "Unit boundaries crossed from a to b.", "t"],
  ["datepart", "unit, date", "A part of the date as a number, e.g. datepart(year, d).", "t"],
  ["eomonth", "date [, months]", "Last day of the month.", "t"],
  ["try_convert", "type, expr [, style]", "expr converted to type, or NULL when it cannot be.", "t"],
  ["try_cast", "expr AS type", "expr cast to type, or NULL when it cannot be.", "t"],
  ["newid", "", "A random uniqueidentifier.", "t"],
  ["scope_identity", "", "The identity value of the last insert in this scope.", "t"],
  ["object_id", "name", "The id of a schema object, NULL when absent.", "t"],
  ["db_name", "[id]", "Name of the current database.", "t"],
  ["json_value", "json, path", "The scalar value at a path such as '$.a.b'.", "t"],
  ["json_query", "json, path", "The object or array at a path.", "t"],
  ["isjson", "text", "1 when the text is valid JSON.", "t"],
  // ClickHouse
  ["uniq", "expr, …", "Approximate number of distinct values.", "c"],
  ["uniqExact", "expr, …", "Exact number of distinct values.", "c"],
  ["countIf", "cond", "Number of rows where cond holds.", "c"],
  ["sumIf", "expr, cond", "Sum of expr over the rows where cond holds.", "c"],
  ["any", "expr", "Some value of the group.", "c"],
  ["argMax", "arg, val", "arg of the row with the largest val.", "c"],
  ["argMin", "arg, val", "arg of the row with the smallest val.", "c"],
  ["groupArray", "expr", "Values collected into an array.", "c"],
  ["quantile", "level)(expr", "Approximate quantile, e.g. quantile(0.9)(x).", "c"],
  ["multiIf", "cond1, then1, …, else", "The value of the first condition that holds.", "c"],
  ["toString", "expr", "expr as text.", "c"],
  ["toDate", "expr", "expr as a Date.", "c"],
  ["toDateTime", "expr [, tz]", "expr as a DateTime.", "c"],
  ["toInt64", "expr", "expr as a 64-bit integer.", "c"],
  ["toStartOfDay", "datetime", "Midnight of the day.", "c"],
  ["toStartOfMonth", "date", "First day of the month.", "c"],
  ["toYYYYMM", "date", "Year and month as a number, e.g. 202403.", "c"],
  ["formatDateTime", "datetime, format", "The date formatted, e.g. '%Y-%m-%d'.", "c"],
  ["dateDiff", "unit, a, b", "Unit boundaries crossed from a to b.", "c"],
  ["today", "", "The current date.", "c"],
  ["arrayJoin", "array", "One row per array element.", "c"],
  ["has", "array, x", "1 when the array holds x.", "c"],
  ["arrayMap", "x -> expr, array", "The array with expr applied to each element.", "c"],
  ["splitByChar", "sep, text", "The parts of text between sep characters.", "c"],
  ["JSONExtractString", "json, key …", "The string at a key path.", "c"],
  ["generateUUIDv4", "", "A random UUID.", "c"],
  ["formatReadableSize", "bytes", "A size such as '1.50 MiB'.", "c"],
];

const LETTER: Record<DriverKind, string> = {
  postgres: "p",
  mysql: "m",
  sqlite: "s",
  duckdb: "d",
  sqlserver: "t",
  clickhouse: "c",
};

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
