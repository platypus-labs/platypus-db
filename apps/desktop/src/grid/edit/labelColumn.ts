/** Which column of a referenced table describes its rows, beside the foreign keys to it. */
import type { ColumnMeta, TableDetails } from "../../api";

/** Names that usually describe a row, best first. */
const LABEL_NAMES = ["name", "title", "label", "description", "descr", "code", "username", "email"];
const TEXT_TYPE = /char|text|string|enum|citext/i;

/**
 * The column that describes the rows of `details`, other than its key `key`: `preferred`
 * when the user picked one of its columns, otherwise the best named or the first text column.
 */
export function labelColumn(details: TableDetails, key: string, preferred?: string): string | null {
  if (preferred !== undefined && preferred !== key && details.columns.some((c) => c.name === preferred)) return preferred;
  const candidates = details.columns.filter((c) => c.name !== key && !c.primary_key);
  const named = (c: ColumnMeta) => LABEL_NAMES.indexOf(c.name.toLowerCase());
  const byName = candidates
    .filter((c) => named(c) >= 0)
    .sort((a, b) => named(a) - named(b))[0];
  return (byName ?? candidates.find((c) => TEXT_TYPE.test(c.data_type)))?.name ?? null;
}
