/**
 * The Markdown of a notebook's text cells: headings, paragraphs, lists, quotes, code blocks,
 * rules, and inline code, bold, italic and links. Parsed to a small tree that the cell renders
 * as React elements and the HTML export as escaped HTML: no text ever becomes markup. The
 * export of the whole notebook as a page is here too. No app imports: the unit tests load it
 * as it is.
 */
import type { PlanLine } from "./model";

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong" | "em"; children: Inline[] }
  /** Only `http`, `https` and `mailto` targets; other links stay text. */
  | { kind: "link"; href: string; children: Inline[] };

export type Block =
  | { kind: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; children: Inline[] }
  | { kind: "paragraph"; children: Inline[] }
  | { kind: "list"; ordered: boolean; items: Inline[][] }
  | { kind: "quote"; children: Block[] }
  | { kind: "code"; lang: string; text: string }
  | { kind: "rule" };

const FENCE = /^\s*(```|~~~)\s*([\w+-]*)\s*$/;
const HEADING = /^\s*(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
const BULLET = /^\s*[-*+]\s+(.*)$/;
const NUMBERED = /^\s*\d+[.)]\s+(.*)$/;
const QUOTE = /^\s*>\s?(.*)$/;

/** Whether `line` starts a block other than a paragraph, which ends the paragraph before. */
const startsBlock = (line: string) =>
  FENCE.test(line) || HEADING.test(line) || RULE.test(line) || BULLET.test(line) || NUMBERED.test(line) || QUOTE.test(line);

export function parseMarkdown(text: string): Block[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    if (!line.trim()) {
      i++;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i]!.trim().startsWith(fence[1]!)) body.push(lines[i++]!);
      i++;
      blocks.push({ kind: "code", lang: fence[2] ?? "", text: body.join("\n") });
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({ kind: "heading", level: heading[1]!.length as 1, children: parseInline(heading[2]!) });
      i++;
      continue;
    }
    if (RULE.test(line)) {
      blocks.push({ kind: "rule" });
      i++;
      continue;
    }
    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i]!)) body.push(QUOTE.exec(lines[i++]!)![1]!);
      blocks.push({ kind: "quote", children: parseMarkdown(body.join("\n")) });
      continue;
    }
    const ordered = NUMBERED.test(line);
    const item = ordered ? NUMBERED : BULLET;
    if (item.test(line)) {
      const items: Inline[][] = [];
      while (i < lines.length && item.test(lines[i]!)) {
        let content = item.exec(lines[i++]!)![1]!;
        // Indented lines continue the item.
        while (i < lines.length && /^\s{2,}\S/.test(lines[i]!) && !startsBlock(lines[i]!)) content += ` ${lines[i++]!.trim()}`;
        items.push(parseInline(content));
      }
      blocks.push({ kind: "list", ordered, items });
      continue;
    }
    const body: string[] = [];
    while (i < lines.length && lines[i]!.trim() && !startsBlock(lines[i]!)) body.push(lines[i++]!.trim());
    blocks.push({ kind: "paragraph", children: parseInline(body.join(" ")) });
  }
  return blocks;
}

const SAFE_HREF = /^(https?:\/\/|mailto:)/i;

/** The inline elements of `text`. */
export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let plain = "";
  const flush = () => {
    if (plain) out.push({ kind: "text", text: plain });
    plain = "";
  };
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    const code = /^(`+)([\s\S]*?[^`])\1(?!`)/.exec(rest);
    if (code) {
      flush();
      out.push({ kind: "code", text: code[2]!.trim() });
      i += code[0].length;
      continue;
    }
    const strong = /^(\*\*|__)(?=\S)([\s\S]*?\S)\1/.exec(rest);
    if (strong) {
      flush();
      out.push({ kind: "strong", children: parseInline(strong[2]!) });
      i += strong[0].length;
      continue;
    }
    const em = /^([*_])(?=\S)([\s\S]*?\S)\1(?![*_\w])/.exec(rest);
    // `snake_case_names` stay text: `_` opens emphasis only at the start of a word.
    if (em && !(em[1] === "_" && /\w/.test(text[i - 1] ?? ""))) {
      flush();
      out.push({ kind: "em", children: parseInline(em[2]!) });
      i += em[0].length;
      continue;
    }
    const link = /^\[([^\]]+)\]\(([^)\s]+)\)/.exec(rest);
    if (link) {
      flush();
      const children = parseInline(link[1]!);
      if (SAFE_HREF.test(link[2]!)) out.push({ kind: "link", href: link[2]!, children });
      else out.push(...children);
      i += link[0].length;
      continue;
    }
    // A backslash keeps the next character as it is.
    if (text[i] === "\\" && i + 1 < text.length && /[\\`*_[\]()#>+\-.!]/.test(text[i + 1]!)) {
      plain += text[i + 1];
      i += 2;
      continue;
    }
    plain += text[i];
    i++;
  }
  flush();
  return out;
}

export const escapeHtml = (text: string) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");

function inlineHtml(nodes: Inline[]): string {
  return nodes
    .map((n) => {
      switch (n.kind) {
        case "text":
          return escapeHtml(n.text);
        case "code":
          return `<code>${escapeHtml(n.text)}</code>`;
        case "strong":
          return `<strong>${inlineHtml(n.children)}</strong>`;
        case "em":
          return `<em>${inlineHtml(n.children)}</em>`;
        case "link":
          return `<a href="${escapeHtml(n.href)}">${inlineHtml(n.children)}</a>`;
      }
    })
    .join("");
}

function blockHtml(block: Block): string {
  switch (block.kind) {
    case "heading":
      return `<h${block.level}>${inlineHtml(block.children)}</h${block.level}>`;
    case "paragraph":
      return `<p>${inlineHtml(block.children)}</p>`;
    case "list": {
      const tag = block.ordered ? "ol" : "ul";
      return `<${tag}>${block.items.map((item) => `<li>${inlineHtml(item)}</li>`).join("")}</${tag}>`;
    }
    case "quote":
      return `<blockquote>${block.children.map(blockHtml).join("")}</blockquote>`;
    case "code":
      return `<pre><code>${escapeHtml(block.text)}</code></pre>`;
    case "rule":
      return "<hr>";
  }
}

/** `text` as HTML, every character of it escaped. */
export function markdownHtml(text: string): string {
  return parseMarkdown(text).map(blockHtml).join("\n");
}

/** A cell as the export writes it. */
export type ExportCell =
  | { kind: "markdown"; text: string }
  | {
      kind: "sql";
      name: string;
      sql: string;
      /** Its rows (as text, `null` for SQL NULL), at most those the export takes. */
      table: { columns: string[]; rows: (string | null)[][] } | null;
      /** What ran: the rows, or the error. */
      summary: string;
      failed: boolean;
      /** The chart as SVG, when shown. */
      chart: string | null;
      plan: PlanLine[] | null;
    };

const STYLE = `body{font:14px/1.5 system-ui,sans-serif;max-width:960px;margin:32px auto;padding:0 16px;color:#1f2328}
pre{background:#f6f8fa;border:1px solid #d0d7de;border-radius:6px;padding:8px 10px;overflow:auto;font:12px ui-monospace,monospace}
code{font:12px ui-monospace,monospace;background:#f6f8fa;padding:1px 4px;border-radius:4px}pre code{padding:0;background:none}
.cell{margin:20px 0}.name{font:600 12px ui-monospace,monospace;color:#57606a}.summary{color:#57606a;font-size:12px}.failed{color:#cf222e}
table{border-collapse:collapse;font-size:12px;margin:8px 0;display:block;overflow:auto}th,td{border:1px solid #d0d7de;padding:3px 8px;text-align:left;white-space:nowrap}
th{background:#f6f8fa}td.null{color:#8c959f;font-style:italic}.plan{font:12px ui-monospace,monospace;color:#57606a}.chart svg{max-width:100%;height:auto}
blockquote{margin:0;padding-left:12px;border-left:3px solid #d0d7de;color:#57606a}`;

function tableHtml(table: NonNullable<Extract<ExportCell, { kind: "sql" }>["table"]>): string {
  const head = table.columns.map((c) => `<th>${escapeHtml(c)}</th>`).join("");
  const body = table.rows
    .map((row) => `<tr>${row.map((v) => (v === null ? '<td class="null">NULL</td>' : `<td>${escapeHtml(v)}</td>`)).join("")}</tr>`)
    .join("\n");
  return `<table><thead><tr>${head}</tr></thead><tbody>\n${body}\n</tbody></table>`;
}

function planHtml(lines: PlanLine[]): string {
  const text = lines
    .map((l) => {
      const figures = [l.rows !== null ? `rows=${l.rows}` : "", l.cost !== null ? `cost=${l.cost}` : ""].filter(Boolean).join(" ");
      return `${"  ".repeat(l.depth)}${l.depth ? "-> " : ""}${l.operation}${l.detail ? ` ${l.detail}` : ""}${figures ? `  (${figures})` : ""}`;
    })
    .join("\n");
  return `<pre class="plan">${escapeHtml(text)}</pre>`;
}

function cellHtml(cell: ExportCell): string {
  if (cell.kind === "markdown") return `<section class="cell text">\n${markdownHtml(cell.text)}\n</section>`;
  const parts = [
    `<div class="name">${escapeHtml(cell.name)}</div>`,
    `<pre><code>${escapeHtml(cell.sql)}</code></pre>`,
    cell.summary ? `<div class="summary${cell.failed ? " failed" : ""}">${escapeHtml(cell.summary)}</div>` : "",
    // The chart is the app's own SVG, of numbers and the result's text escaped by it.
    cell.chart ? `<div class="chart">${cell.chart}</div>` : cell.table ? tableHtml(cell.table) : "",
    cell.plan ? planHtml(cell.plan) : "",
  ];
  return `<section class="cell sql">\n${parts.filter(Boolean).join("\n")}\n</section>`;
}

/** The notebook as a standalone HTML page, with the values its `params` had. */
export function notebookHtml(title: string, cells: readonly ExportCell[], params: Readonly<Record<string, string>> = {}): string {
  const values = Object.entries(params)
    .map(([key, value]) => `<code>${escapeHtml(key.startsWith("?") ? key : `:${key}`)}</code> = ${escapeHtml(value)}`)
    .join(", ");
  return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>${escapeHtml(title)}</title>
<style>${STYLE}</style>
</head>
<body>
<h1>${escapeHtml(title)}</h1>
${values ? `<p class="summary">${values}</p>\n` : ""}${cells.map(cellHtml).join("\n")}
</body>
</html>
`;
}
