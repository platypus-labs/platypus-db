/** An ER diagram as the text of an SVG file, drawn from a model alone: laid out and routed with
 *  the code the diagram tab uses, sized by `geometry.ts`. No DOM and no React are involved, and
 *  nothing is parsed: every name is untrusted text and goes into the image escaped, so the
 *  result holds no script, no event attribute and no markup of the tables' making. */
import type { ChartColors } from "../analysis/Chart";
import { truncate } from "../analysis/chartScale.ts";
import { CHAR, HEAD, MARK, MARK_BOTH, PAD, ROW, type Shape, boxOf, fkColumns, local, nameChars, rowY, typeText } from "./geometry.ts";
import { autoLayout, type Point } from "./layout.ts";
import { pathThrough, routeWires } from "./route.ts";

/** Characters XML 1.0 cannot hold (controls, the two non-characters, lone surrogates). */
// eslint-disable-next-line no-control-regex
const INVALID_XML = /[\u0000-\u0008\u000B\u000C\u000E-\u001F￾￿]|[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g;

/** `text` safe in element content and in a quoted attribute value. */
export function escapeXml(text: string): string {
  return text
    .replace(INVALID_XML, "�")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** A colour from the theme, or `fallback` when it holds anything but what a colour is made of. */
const colour = (value: string | undefined, fallback: string) => (value && /^[#\w(),.%\s/+-]+$/.test(value) ? value : fallback);

const num = (n: number) => String(Math.round(n * 100) / 100);

/** The SVG of the diagram of `tables`, which belong to `schema`: a key to a table of another
 *  schema, or not in `tables`, is not drawn. */
export function erdSvg(tables: Shape[], schema: string, colors: ChartColors): string {
  const c = {
    text: colour(colors.text, "#1f2328"),
    muted: colour(colors.muted, "#656d76"),
    grid: colour(colors.grid, "#d0d7de"),
    background: colour(colors.background, "#ffffff"),
    pk: colour(colors.series[0], "#0969da"),
    fk: colour(colors.series[4], "#8250df"),
  };
  const boxes = tables.map((t) => boxOf(t, schema));
  const at = autoLayout(boxes);
  const pos = (name: string): Point => at[name] ?? { x: PAD, y: PAD };
  const byName = new Map(tables.map((t) => [t.name, t]));
  const rects = Object.fromEntries(boxes.map((b) => [b.name, { ...pos(b.name), width: b.width, height: b.height }]));
  const keys = tables.flatMap((table) =>
    table.foreign_keys.flatMap((fk) => {
      const target = local(fk, schema) ? byName.get(fk.ref_table) : undefined;
      return target ? [{ table, target, fk }] : [];
    }),
  );
  const lines = routeWires(
    rects,
    keys.map(({ table, target, fk }) => ({ from: table.name, fromRow: rowY(table, fk.columns[0]), to: target.name, toRow: rowY(target, fk.ref_columns[0]) })),
  );
  let w = 400;
  let h = 300;
  for (const b of boxes) {
    w = Math.max(w, pos(b.name).x + b.width + PAD);
    h = Math.max(h, pos(b.name).y + b.height + PAD);
  }
  for (const line of lines) {
    for (const p of line) {
      w = Math.max(w, p.x + PAD);
      h = Math.max(h, p.y + PAD);
    }
  }
  w = Math.ceil(w);
  h = Math.ceil(h);
  const out = [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" font-family="${escapeXml(colors.font || "sans-serif")}" font-size="12">`,
    `<defs><marker id="one" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="10" markerHeight="10" orient="auto-start-reverse"><path d="M 6 0 L 6 10" stroke="${escapeXml(c.muted)}" fill="none"/></marker>` +
      `<marker id="many" viewBox="0 0 10 10" refX="1" refY="5" markerWidth="10" markerHeight="10" orient="auto-start-reverse"><path d="M 10 5 L 0 0 M 10 5 L 0 10 M 10 5 L 0 5" stroke="${escapeXml(c.muted)}" fill="none"/></marker></defs>`,
    `<rect width="${w}" height="${h}" fill="${escapeXml(c.background)}"/>`,
  ];
  for (const line of lines) {
    out.push(`<path d="${pathThrough(line)}" stroke="${escapeXml(c.muted)}" fill="none" marker-start="url(#many)" marker-end="url(#one)"/>`);
  }
  for (const [i, table] of tables.entries()) out.push(tableSvg(table, boxes[i]!.width, pos(table.name), c));
  out.push("</svg>");
  return out.join("\n");
}

/** The look of a table's box, in the colours above. */
type Look = { text: string; muted: string; grid: string; background: string; pk: string; fk: string };

function tableSvg(table: Shape, width: number, at: Point, c: Look): string {
  const keys = fkColumns(table);
  const height = HEAD + ROW * table.columns.length + 6;
  const q = (s: string) => escapeXml(s);
  const rows = table.columns.map((col, i) => {
    const both = col.primary_key && keys.has(col.name);
    const mark = both ? MARK_BOTH : MARK;
    const label = truncate(col.name, nameChars(width, typeText(col.data_type), mark) - (col.nullable ? 0 : 2)) + (col.nullable ? "" : " *");
    return (
      `<g transform="translate(0,${HEAD + i * ROW})">` +
      (col.primary_key ? keyMark("PK", c.pk, 0) : "") +
      (keys.has(col.name) ? keyMark("FK", c.fk, col.primary_key ? MARK - 2 : 0) : "") +
      `<text x="${mark + 6}" y="15" fill="${q(col.primary_key ? c.pk : c.text)}"${col.primary_key ? ' font-weight="600"' : ""}>${q(label)}</text>` +
      `<text x="${width - 10}" y="15" fill="${q(c.muted)}" font-size="11" text-anchor="end">${q(typeText(col.data_type))}</text></g>`
    );
  });
  return [
    `<g class="erd-table" transform="translate(${num(at.x)},${num(at.y)})">`,
    `<rect width="${width}" height="${height}" rx="4" fill="${q(c.background)}" stroke="${q(c.grid)}"/>`,
    `<rect width="${width}" height="${HEAD}" rx="4" fill="${q(c.pk)}" fill-opacity="0.14"/>`,
    `<path d="M 0 ${HEAD} H ${width}" stroke="${q(c.grid)}"/>`,
    `<text x="10" y="18" fill="${q(c.text)}" font-weight="600">${q(truncate(table.name, Math.max(4, Math.floor((width - 20) / CHAR))))}</text>`,
    ...rows,
    "</g>",
  ].join("\n");
}

/** The small PK or FK mark at the start of a column row. */
function keyMark(label: string, fill: string, at: number): string {
  const f = escapeXml(fill);
  return (
    `<g transform="translate(${at},0)"><rect x="6" y="4" width="${MARK - 4}" height="14" rx="3" fill="${f}" fill-opacity="0.18"/>` +
    `<text x="${6 + (MARK - 4) / 2}" y="14" fill="${f}" font-size="9" font-weight="600" text-anchor="middle">${label}</text></g>`
  );
}
