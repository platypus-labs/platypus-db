/** The words for the proposed rows the data editor could not apply. */
import { plural, t } from "../../i18n";
import type { Skip } from "./proposed";

/** Rows named in the notice; the rest are counted. */
const SHOWN = 4;

function why(skip: Skip): string {
  const column = skip.column ?? "";
  switch (skip.reason) {
    case "no_row":
      return t("no row of the table has that key");
    case "many_rows":
      return t("several rows have that key");
    case "missing_column":
      return t("the column {column} is not in the result", { column });
    case "database":
      return t("the column {column} is filled in by the database", { column });
    case "not_table":
      return t("{column} is not a column of the table", { column });
    case "value":
      return t("the value for {column} does not fit its type", { column });
  }
}

function what(skip: Skip): string {
  const n = skip.index + 1;
  if (skip.kind === "update") return t("Update {n}", { n });
  return skip.kind === "delete" ? t("Delete {n}", { n }) : t("New row {n}", { n });
}

/** One sentence naming the first rows left out and why. */
export function skippedNotice(skipped: readonly Skip[]): string {
  const named = skipped.slice(0, SHOWN).map((s) => t("{what}: {why}", { what: what(s), why: why(s) }));
  if (skipped.length > SHOWN) named.push(t("and {n} more", { n: skipped.length - SHOWN }));
  return plural(
    skipped.length,
    "{n} proposed row could not be applied. {list}",
    "{n} proposed rows could not be applied. {list}",
    { list: named.join("; ") },
  );
}
