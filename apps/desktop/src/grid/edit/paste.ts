/**
 * Clipboard text as rows of cells. Tabs separate the cells and line breaks the rows, as
 * spreadsheets (Excel, LibreOffice, Google Sheets) and the grid's own copy write them; nothing
 * else splits, so a value with commas or semicolons ("Paris, France", a JSON object) stays one.
 * The grid's own copy also comes back with its NULLs, which the text cannot tell from empty
 * strings.
 */

/** A field in double quotes (`""` is a quote) starting at `start`: its value and the index past
 *  its closing quote, when that quote ends the field. `null` when the text is not one, e.g.
 *  `"quoted" word`: such a field is taken as it is. */
function quotedField(text: string, start: number): { value: string; end: number } | null {
  let value = "";
  for (let i = start + 1; i < text.length; i++) {
    const c = text[i]!;
    if (c !== '"') {
      value += c;
    } else if (text[i + 1] === '"') {
      value += '"';
      i++;
    } else {
      const next = text[i + 1];
      return next === undefined || next === "\t" || next === "\n" || next === "\r" ? { value, end: i + 1 } : null;
    }
  }
  return null;
}

/** Splits tab-separated text into rows of fields. */
function split(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let fresh = true;
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    if (c === '"' && fresh) {
      const quoted = quotedField(text, i);
      if (quoted) {
        field = quoted.value;
        i = quoted.end - 1;
        fresh = false;
        continue;
      }
    }
    fresh = false;
    if (c === "\t") {
      row.push(field);
      field = "";
      fresh = true;
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
      fresh = true;
    } else {
      field += c;
    }
  }
  if (!fresh || row.length > 0) {
    row.push(field);
    rows.push(row);
  }
  return rows;
}

/** Cells as clipboard text: tab-separated lines, NULL as nothing. One cell is its value as it
 *  is; among several, a value with a tab, a line break or a leading quote goes in quotes, so
 *  that spreadsheets and the paste above read it back whole. */
export function clipboardText(rows: readonly (readonly (string | null)[])[]): string {
  const single = rows.length === 1 && rows[0]!.length === 1;
  const field = (v: string | null) =>
    v === null ? "" : !single && /^"|[\t\r\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v;
  return rows.map((cells) => cells.map(field).join("\t")).join("\n");
}

/** The text and cells of the grid's last copy (`null` is NULL). */
let lastCopy: { text: string; rows: (string | null)[][] } | null = null;

/** Line breaks as `\n`, without the one at the end that some clipboards add. */
const normalized = (text: string) => text.replace(/\r\n?/g, "\n").replace(/\n$/, "");

/** Remembers the cells the grid put on the clipboard as `text`, to paste them back as they
 *  were. */
export function rememberCopy(text: string, rows: (string | null)[][]): void {
  lastCopy = { text: normalized(text), rows };
}

/** Whether the clipboard's `text` is still the grid's last copy. */
export const isOwnCopy = (text: string): boolean => lastCopy !== null && lastCopy.text === normalized(text);

/** The clipboard's `text` as rows of cells; `null` cells (NULL) come only from the grid's own
 *  copy, when the clipboard still holds it. */
export function parseClipboard(text: string): (string | null)[][] {
  if (lastCopy && isOwnCopy(text)) return lastCopy.rows;
  if (!/[\t\r\n]/.test(text)) return [[text]];
  return split(text);
}

/**
 * Where pasted fields go: by name when the first row repeats column names (a copy with
 * headers), else to the columns of `order` in turn (the ones shown from the focused cell
 * on). Returns the target column of each field (`-1` skips it) and the rows without the
 * header.
 */
export function mapFields<T extends string | null>(
  rows: T[][],
  names: string[],
  order: number[],
): { targets: number[]; rows: T[][] } {
  const header = rows[0] ?? [];
  const index = new Map(names.map((n, i) => [n.toLowerCase(), i]));
  const column = (h: T) => (h === null ? undefined : index.get(h.trim().toLowerCase()));
  if (rows.length > 1 && header.length > 0 && header.every((h) => column(h) !== undefined)) {
    return { targets: header.map((h) => column(h)!), rows: rows.slice(1) };
  }
  const width = Math.max(0, ...rows.map((r) => r.length));
  return {
    targets: Array.from({ length: width }, (_, i) => order[i] ?? -1),
    rows,
  };
}
