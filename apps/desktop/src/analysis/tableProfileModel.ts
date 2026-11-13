/** What the table profile tab decides without the screen: the sample sizes, the filter box, and
 *  the conditions that open the table's data on a value or on the outliers. Pure: the unit
 *  tests import it. */
import type { DriverKind, TableColumnProfile } from "../api";

/** Rows to read, as the Sample select offers them; `null` reads the whole table. */
const SAMPLES: (number | null)[] = [10_000, 100_000, 1_000_000, null];
/** What the select starts with, and what a tab saved by another version falls back to. */
export const DEFAULT_SAMPLE = 100_000;

/** A saved sample size, or the default when it is not one of [`SAMPLES`]. */
export function restoreSample(saved: unknown): number | null {
  if (saved === null) return null;
  return typeof saved === "number" && SAMPLES.includes(saved) ? saved : DEFAULT_SAMPLE;
}

/** The columns whose name or type contains `filter`, ignoring case; all for a blank filter. */
export function matchingColumns(columns: TableColumnProfile[], filter: string): TableColumnProfile[] {
  const needle = filter.trim().toLowerCase();
  if (!needle) return columns;
  return columns.filter((c) => c.name.toLowerCase().includes(needle) || (c.db_type ?? c.data_type).toLowerCase().includes(needle));
}

/** Drivers whose string literals treat a backslash as an escape: a value with one is not
 *  written as a literal here. */
const BACKSLASH_ESCAPES: DriverKind[] = ["mysql", "clickhouse", "bigquery", "databricks"];

/** `column = 'value'` for a `column` already quoted, or `null` where the literal cannot be
 *  written safely (a backslash on a driver that escapes with it, a NUL character). */
export function equalsCondition(driver: DriverKind, column: string, value: string): string | null {
  if (value.includes("\0")) return null;
  if (value.includes("\\") && BACKSLASH_ESCAPES.includes(driver)) return null;
  return `${column} = '${value.replaceAll("'", "''")}'`;
}

/** The rows with an outlier in `column` (already quoted): below the low fence or above the
 *  high one. */
export function outlierCondition(column: string, low: number, high: number): string {
  return `(${column} < ${low} OR ${column} > ${high})`;
}
