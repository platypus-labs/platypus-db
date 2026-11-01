// Type-ahead in the navigator tree: printable keys typed in quick succession pick the next row
// whose label starts with them. Pure, so it can be tested without the DOM.

/** Milliseconds without typing after which the typed text starts over. */
export const TYPE_AHEAD_RESET_MS = 1000;

/** Whether `e` is a plain printable key: no Ctrl/Alt/Meta, one character. */
export function isTypeAheadKey(e: { key: string; ctrlKey: boolean; altKey: boolean; metaKey: boolean }): boolean {
  return e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey;
}

/** The text typed after `key`: appended to `typed`, or starting over after a pause. */
export function nextTyped(typed: string, key: string, sinceLast: number): string {
  return (sinceLast > TYPE_AHEAD_RESET_MS ? "" : typed) + key;
}

/**
 * The index of the row to move to: the first of `labels` after `from` (wrapping, `from` last)
 * that starts with `typed`, case-insensitively; -1 when none does. One letter repeated ("aa")
 * searches for that letter alone, so it cycles through the rows starting with it.
 */
export function typeAheadIndex(labels: string[], from: number, typed: string): number {
  const first = typed[0]?.toLowerCase() ?? "";
  const repeated = typed.length > 1 && [...typed].every((c) => c.toLowerCase() === first);
  const prefix = repeated ? first : typed.toLowerCase();
  for (let i = 1; i <= labels.length; i++) {
    const at = (from + i) % labels.length;
    if (labels[at]!.toLowerCase().startsWith(prefix)) return at;
  }
  return -1;
}
