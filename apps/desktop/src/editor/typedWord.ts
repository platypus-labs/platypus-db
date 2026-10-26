// Whether Enter after a word means a newline rather than accepting the selected completion.
// Plain logic, tested on its own (tests/typedWord.test.mjs).

/** The word before the cursor: letters and digits of any script (with their accents), `_`,
 *  `$` and `#`. */
export function wordBefore(text: string): string | undefined {
  return /[\p{L}\p{M}\p{N}_$#]+$/u.exec(text)?.[0];
}

/** Whether accepting `entry` would leave `word` exactly as typed: the same text in the same
 *  case, and nothing else inserted (no quotes, parentheses or snippet). */
export function typedInFull(word: string | undefined, entry: { label: string; apply?: unknown } | null): boolean {
  if (!word || !entry || word !== entry.label) return false;
  return entry.apply === undefined || entry.apply === entry.label;
}
