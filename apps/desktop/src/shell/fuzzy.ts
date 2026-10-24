/**
 * Fuzzy matching for the command palette: the query's characters must appear in order in
 * the text, ignoring case. Matches at word starts and in a row score higher, so "otb" finds
 * "Open table" before "bottom".
 */

export interface FuzzyMatch {
  score: number;
  /** Indices of the matched characters, to highlight them. */
  positions: number[];
}

const isWordStart = (text: string, i: number) => i === 0 || /[\s._\-/:]/.test(text[i - 1]!) || (/[a-z]/.test(text[i - 1]!) && /[A-Z]/.test(text[i]!));

export function fuzzy(query: string, text: string): FuzzyMatch | null {
  const q = query.trim().toLowerCase();
  if (!q) return { score: 0, positions: [] };
  const t = text.toLowerCase();
  const positions: number[] = [];
  let score = 0;
  let from = 0;
  for (const c of q) {
    if (c === " ") continue;
    const i = t.indexOf(c, from);
    if (i < 0) return null;
    const previous = positions.at(-1);
    score += 1;
    if (previous !== undefined && i === previous + 1) score += 3;
    if (isWordStart(text, i)) score += 4;
    // Gaps cost a little, so tighter matches win.
    score -= Math.min(3, (i - from) * 0.1);
    positions.push(i);
    from = i + 1;
  }
  // Shorter texts with the same matches are closer.
  return { score: score - text.length * 0.01, positions };
}
