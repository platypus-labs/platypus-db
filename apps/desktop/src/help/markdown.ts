/**
 * The Markdown of the guide pages: headings, paragraphs, nested lists, quotes, code blocks,
 * tables, rules, and inline code, bold, italic, links and images. Parsed to a small tree that
 * the dialog renders as React elements, so no text ever becomes markup; raw HTML lines are
 * dropped. No app imports: the unit tests load it as it is.
 */
import { slugger } from "./pages.ts";

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong" | "em"; children: Inline[] }
  | { kind: "link"; href: string; children: Inline[] }
  | { kind: "image"; src: string; alt: string };

export type Align = "left" | "center" | "right" | null;

export type Block =
  | { kind: "heading"; level: number; children: Inline[]; anchor: string }
  | { kind: "paragraph"; children: Inline[] }
  | { kind: "list"; ordered: boolean; start: number; items: Block[][] }
  | { kind: "quote"; children: Block[] }
  | { kind: "code"; lang: string; text: string }
  | { kind: "table"; header: Inline[][]; align: Align[]; rows: Inline[][][] }
  | { kind: "rule" };

const FENCE = /^\s*(```+|~~~+)\s*([\w+-]*)\s*$/;
const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)(?:\s+#+)?\s*$/;
const RULE = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
const ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
const QUOTE = /^\s{0,3}>\s?(.*)$/;
const HTML_LINE = /^\s*(<!--.*-->|<\/?[a-zA-Z][^>]*>)+\s*$/;
const TABLE_RULE = /^\s*\|?\s*:?-{1,}:?\s*(\|\s*:?-{1,}:?\s*)*\|?\s*$/;

const indentOf = (line: string) => /^\s*/.exec(line)![0].replace(/\t/g, "    ").length;
const isBlank = (line: string) => line.trim() === "";

/** Whether `line` starts a block other than a paragraph. */
const startsBlock = (line: string) =>
  FENCE.test(line) || HEADING.test(line) || RULE.test(line) || ITEM.test(line) || QUOTE.test(line) || HTML_LINE.test(line);

/** The cells of a table row. */
function cellsOf(line: string): string[] {
  const text = line.trim().replace(/^\|/, "").replace(/(?<!\\)\|$/, "");
  const cells: string[] = [];
  let cell = "";
  let tick = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    if (c === "`") tick = !tick;
    if (c === "\\" && text[i + 1] === "|") {
      cell += "|";
      i++;
    } else if (c === "|" && !tick) {
      cells.push(cell.trim());
      cell = "";
    } else cell += c;
  }
  cells.push(cell.trim());
  return cells;
}

const alignOf = (cell: string): Align => {
  const left = cell.startsWith(":");
  const right = cell.endsWith(":");
  return left && right ? "center" : right ? "right" : left ? "left" : null;
};

/** Parses a list starting at `lines[at]`; returns the block and the line after it. */
function parseList(lines: string[], at: number, anchors: (text: string) => string): [Block, number] {
  const first = ITEM.exec(lines[at]!)!;
  const ordered = /\d/.test(first[2]!);
  const base = indentOf(lines[at]!);
  const items: Block[][] = [];
  let i = at;
  while (i < lines.length) {
    const match = ITEM.exec(lines[i]!);
    if (!match || indentOf(lines[i]!) > base + 1 || /\d/.test(match[2]!) !== ordered) break;
    const offset = indentOf(lines[i]!) + match[2]!.length + 1;
    const body = [match[3]!];
    i++;
    while (i < lines.length) {
      const line = lines[i]!;
      if (isBlank(line)) {
        // A blank line belongs to the item only when what follows is indented under it.
        let next = i + 1;
        while (next < lines.length && isBlank(lines[next]!)) next++;
        if (next < lines.length && indentOf(lines[next]!) >= offset) {
          body.push("");
          i++;
          continue;
        }
        break;
      }
      if (indentOf(line) >= offset || (indentOf(line) > base && !startsBlock(line))) {
        body.push(line.replace(/\t/g, "    ").slice(Math.min(offset, indentOf(line))));
        i++;
      } else break;
    }
    items.push(parseBlocks(body, anchors));
    while (i < lines.length && isBlank(lines[i]!) && ITEM.test(lines[i + 1] ?? "") && indentOf(lines[i + 1]!) <= base + 1) i++;
  }
  return [{ kind: "list", ordered, start: ordered ? Number(first[2]!.replace(/\D/g, "")) : 1, items }, i];
}

function parseBlocks(lines: string[], anchors: (text: string) => string): Block[] {
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    if (isBlank(line) || HTML_LINE.test(line)) {
      i++;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      const body: string[] = [];
      const indent = indentOf(line);
      i++;
      while (i < lines.length && !(lines[i]!.trim().startsWith(fence[1]!) && lines[i]!.trim().replace(/[`~]/g, "") === "")) {
        body.push(lines[i]!.slice(Math.min(indent, indentOf(lines[i]!))));
        i++;
      }
      i++;
      blocks.push({ kind: "code", lang: fence[2] ?? "", text: body.join("\n") });
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      const children = parseInline(heading[2]!);
      blocks.push({ kind: "heading", level: heading[1]!.length, children, anchor: anchors(plainText(children)) });
      i++;
      continue;
    }
    if (RULE.test(line) && !ITEM.test(line)) {
      blocks.push({ kind: "rule" });
      i++;
      continue;
    }
    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i]!)) body.push(QUOTE.exec(lines[i++]!)![1]!);
      blocks.push({ kind: "quote", children: parseBlocks(body, anchors) });
      continue;
    }
    if (ITEM.test(line)) {
      const [list, next] = parseList(lines, i, anchors);
      blocks.push(list);
      i = next;
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]!) && lines[i + 1]!.includes("-")) {
      const header = cellsOf(line);
      const align = cellsOf(lines[i + 1]!).map(alignOf);
      i += 2;
      const rows: Inline[][][] = [];
      while (i < lines.length && !isBlank(lines[i]!) && lines[i]!.includes("|")) {
        const cells = cellsOf(lines[i++]!);
        rows.push(header.map((_, c) => parseInline(cells[c] ?? "")));
      }
      blocks.push({ kind: "table", header: header.map((h) => parseInline(h)), align, rows });
      continue;
    }
    const body: string[] = [];
    while (i < lines.length && !isBlank(lines[i]!) && (body.length === 0 || !startsBlock(lines[i]!))) body.push(lines[i++]!.trim());
    blocks.push({ kind: "paragraph", children: parseInline(body.join(" ")) });
  }
  return blocks;
}

/** The blocks of the Markdown `text`. */
export function parseMarkdown(text: string): Block[] {
  return parseBlocks(text.replace(/\r\n?/g, "\n").split("\n"), slugger());
}

/** The text of inline nodes, as a heading's anchor is made from it. */
export function plainText(nodes: Inline[]): string {
  return nodes
    .map((n) => {
      switch (n.kind) {
        case "text":
        case "code":
          return n.text;
        case "image":
          return n.alt;
        default:
          return plainText(n.children);
      }
    })
    .join("");
}

/** The index just after the `]` that closes the `[` at `open`, or -1. */
function closingBracket(text: string, open: number): number {
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    const c = text[i]!;
    if (c === "\\") i++;
    else if (c === "`") {
      const end = text.indexOf("`", i + 1);
      if (end > 0) i = end;
    } else if (c === "[") depth++;
    else if (c === "]" && --depth === 0) return i;
  }
  return -1;
}

/** The address in `(...)` at `at`, with its end, or `null`. One level of parentheses is allowed in it. */
function target(text: string, at: number): { href: string; end: number } | null {
  if (text[at] !== "(") return null;
  const match = /^\(\s*(<[^>]*>|(?:[^()\s]|\([^()\s]*\))*)(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)/.exec(text.slice(at));
  if (!match) return null;
  return { href: match[1]!.replace(/^<|>$/g, ""), end: at + match[0].length };
}

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
    const isImage = text[i] === "!" && text[i + 1] === "[";
    if (isImage || text[i] === "[") {
      const open = isImage ? i + 1 : i;
      const close = closingBracket(text, open);
      const to = close > 0 ? target(text, close + 1) : null;
      if (to) {
        flush();
        const label = text.slice(open + 1, close);
        if (isImage) out.push({ kind: "image", src: to.href, alt: label });
        else out.push({ kind: "link", href: to.href, children: parseInline(label) });
        i = to.end;
        continue;
      }
    }
    const auto = /^<((?:https?:\/\/|mailto:)[^\s<>]+)>/i.exec(rest);
    if (auto) {
      flush();
      out.push({ kind: "link", href: auto[1]!, children: [{ kind: "text", text: auto[1]! }] });
      i += auto[0].length;
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
    if (text[i] === "\\" && i + 1 < text.length && /[\\`*_{}[\]()#>+\-.!|<]/.test(text[i + 1]!)) {
      plain += text[i + 1];
      i += 2;
      continue;
    }
    // Inline HTML (`<kbd>`, `<br>`) is not markup here: the tag is dropped, its text stays.
    const tag = /^<\/?[a-zA-Z][^<>]*>/.exec(rest);
    if (tag) {
      i += tag[0].length;
      continue;
    }
    plain += text[i];
    i++;
  }
  flush();
  return out;
}
