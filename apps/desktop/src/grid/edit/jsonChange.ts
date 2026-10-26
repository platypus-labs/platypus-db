/** Removes the whitespace outside strings, so two spellings of one JSON text compare equal. */
function stripLayout(text: string): string {
  let out = "";
  let inString = false;
  let escaped = false;
  for (const ch of text) {
    if (inString) {
      out += ch;
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') inString = false;
    } else if (ch === '"') {
      inString = true;
      out += ch;
    } else if (!/\s/.test(ch)) {
      out += ch;
    }
  }
  return out;
}

/**
 * Whether `text` is the same JSON as `original` apart from layout (indentation, line breaks):
 * formatting a value for reading is not a change. Numbers and strings are compared as
 * written, so no precision is lost to parsing.
 */
export function sameJson(original: string | null, text: string): boolean {
  if (original === null) return false;
  try {
    JSON.parse(original);
    JSON.parse(text);
  } catch {
    return false;
  }
  return stripLayout(original) === stripLayout(text);
}
