/** Matching what the user types in a search field against labels. Pure: unit tested. */

/** Lower case without accents. */
export const fold = (text: string) => text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

const WORD_CHAR = /[\p{L}\p{N}]/u;

/** Whether `needle`, already folded, starts a word of `text` (case and accents ignored):
 *  "tab" finds "Tabs" and "table", not "database" nor "comfortable". */
export function startsWord(text: string, needle: string): boolean {
  if (!needle) return true;
  const hay = fold(text);
  for (let at = hay.indexOf(needle); at !== -1; at = hay.indexOf(needle, at + 1)) {
    if (at === 0 || !WORD_CHAR.test(hay[at - 1]!)) return true;
  }
  return false;
}
