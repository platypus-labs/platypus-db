/**
 * Clipboard text as rows of cells: tab-separated from spreadsheets (Excel, LibreOffice,
 * Google Sheets) or CSV with commas or semicolons, quoted fields included.
 */

/** Splits delimited text, honouring double quotes around fields (`""` is a quote). */
function split(text: string, delimiter: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    if (quoted) {
      if (c !== '"') field += c;
      else if (text[i + 1] === '"') {
        field += '"';
        i++;
      } else quoted = false;
    } else if (c === '"' && field === "") {
      quoted = true;
    } else if (c === delimiter) {
      row.push(field);
      field = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
    } else {
      field += c;
    }
  }
  if (field !== "" || row.length > 0) {
    row.push(field);
    rows.push(row);
  }
  return rows;
}

/** The delimiter of the first line: tabs win, then semicolons, then commas. */
function delimiterOf(text: string): string {
  const line = text.slice(0, text.search(/\r|\n|$/));
  if (line.includes("\t")) return "\t";
  const count = (d: string) => line.split(d).length;
  return count(";") > count(",") ? ";" : ",";
}

export function parseClipboard(text: string): string[][] {
  return split(text, delimiterOf(text));
}

/**
 * Where pasted fields go: by name when the first row repeats column names (a copy with
 * headers), else in order from `first`. Returns the target column of each field (`-1`
 * skips it) and the rows without the header.
 */
export function mapFields(
  rows: string[][],
  names: string[],
  first: number,
): { targets: number[]; rows: string[][] } {
  const header = rows[0] ?? [];
  const index = new Map(names.map((n, i) => [n.toLowerCase(), i]));
  if (rows.length > 1 && header.length > 0 && header.every((h) => index.has(h.trim().toLowerCase()))) {
    return { targets: header.map((h) => index.get(h.trim().toLowerCase())!), rows: rows.slice(1) };
  }
  const width = Math.max(0, ...rows.map((r) => r.length));
  return {
    targets: Array.from({ length: width }, (_, i) => (first + i < names.length ? first + i : -1)),
    rows,
  };
}
