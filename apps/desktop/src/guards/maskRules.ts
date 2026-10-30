/** Which columns hold data to hide while the screen is shared: names and types matched by
 *  patterns, `*` for any characters, case ignored. */

/** What a masked value shows: the same for every value, so that not even its length shows. */
export const MASK = "••••••";

export const DEFAULT_NAMES = "*password*, *passwd*, *secret*, *token*, *api_key*, *email*, *e_mail*, *phone*, *mobile*, *ssn*, *iban*, *card_number*, *cvv*, *tax_code*, *fiscal*, *salary*, *birth*";

/** The patterns of a comma-separated list, each a regular expression for a whole name. */
export function patterns(list: string): RegExp[] {
  return list
    .split(",")
    .map((p) => p.trim().toLowerCase())
    .filter(Boolean)
    .map((p) => new RegExp(`^${p.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*")}$`));
}

/** Whether a column, by its name or its type (without size: `varchar(20)` is `varchar`), is
 *  one to mask. */
export function masksColumn(column: { name: string; db_type: string }, names: RegExp[], types: RegExp[]): boolean {
  const name = column.name.toLowerCase();
  const type = column.db_type.toLowerCase().replace(/\(.*$/, "").trim();
  return names.some((p) => p.test(name)) || types.some((p) => p.test(type));
}

/** Whether a column is sensitive: the user's `mark` on it when there is one, else the rules. */
export function isSensitive(
  mark: boolean | null | undefined,
  column: { name: string; db_type: string },
  names: RegExp[],
  types: RegExp[],
): boolean {
  return mark ?? masksColumn(column, names, types);
}
