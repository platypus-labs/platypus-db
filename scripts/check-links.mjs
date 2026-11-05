// Checks the relative links of the documentation: in every Markdown file tracked by git and in the
// HTML of the site (site/), a link to a file must find it and a link to a heading (`#anchor`) must
// find the heading, in the page itself or in the file it names. External addresses (http, https,
// mailto, ...) are not checked. Run by hand from anywhere:
//
//   node scripts/check-links.mjs
//
// Exits 1 and lists each broken link as `file:line: target (why)`.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { decode, slugger } from "../apps/desktop/src/help/pages.ts";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Paths in git's own style (`/`), without those of vendored code. */
const tracked = execFileSync("git", ["ls-files", "-z"], { cwd: root, encoding: "utf8", maxBuffer: 1 << 28 })
  .split("\0")
  .filter((f) => f && !f.startsWith("vendor/") && existsSync(join(root, f)));
const markdown = tracked.filter((f) => /\.md$/i.test(f));
const html = tracked.filter((f) => /^site\/.*\.html?$/i.test(f));

const SCHEME = /^[a-z][a-z0-9+.-]*:/i;

/** The text with fenced code blocks and inline code blanked out (line breaks kept), so a link
 *  written as an example is not read. */
function withoutCode(text) {
  const blank = (s) => s.replace(/[^\n]/g, " ");
  return text.replace(/^(\s*)(```+|~~~+)[^\n]*\n[\s\S]*?(^\s*\2[`~]*\s*$|(?![\s\S]))/gm, blank).replace(/(`+)[^`\n]([^`]|\n(?!\n))*?\1/g, blank);
}

/** A heading's text as it shows: links, emphasis and code marks removed. */
function headingText(raw) {
  return raw
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/[`*~]/g, "")
    .trim();
}

/** The anchors a Markdown file offers: its headings (GitHub's rule) and its `id`/`name` attributes. */
function markdownAnchors(text) {
  const slug = slugger();
  const anchors = new Set();
  const plain = withoutCode(text);
  for (const m of plain.matchAll(/^ {0,3}#{1,6}[ \t]+(.+?)(?:[ \t]+#+)?[ \t]*$/gm)) anchors.add(slug(headingText(m[1])));
  for (const m of plain.matchAll(/<[a-z][^>]*\s(?:id|name)\s*=\s*["']([^"']+)["']/gi)) anchors.add(m[1]);
  return anchors;
}

/** The anchors an HTML file offers: its `id` and `name` attributes. */
function htmlAnchors(text) {
  const anchors = new Set();
  for (const m of text.matchAll(/\s(?:id|name)\s*=\s*["']([^"']+)["']/gi)) anchors.add(m[1]);
  return anchors;
}

const anchorCache = new Map();
/** The anchors of a file (a set), or `null` for a file that has none to check (not Markdown or HTML). */
function anchorsOf(file) {
  if (!anchorCache.has(file)) {
    const text = () => readFileSync(join(root, file), "utf8");
    anchorCache.set(file, /\.md$/i.test(file) ? markdownAnchors(text()) : /\.html?$/i.test(file) ? htmlAnchors(text()) : null);
  }
  return anchorCache.get(file);
}

/** Every link of a Markdown file: `[text](target)`, `![alt](target)`, `[ref]: target`, and the
 *  `href`/`src` attributes of its HTML, each with its line. */
function markdownLinks(text) {
  const plain = withoutCode(text);
  const out = [];
  const add = (index, target) => out.push({ line: plain.slice(0, index).split("\n").length, target });
  // One level of parentheses in the address, as in Foo_(bar); an optional "title" after it.
  for (const m of plain.matchAll(/\]\(\s*(<[^>\n]*>|(?:[^()\s]|\([^()\s]*\))*)(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)/g)) add(m.index, m[1].replace(/^<|>$/g, ""));
  for (const m of plain.matchAll(/^ {0,3}\[[^\]\n]+\]:[ \t]*(<[^>\n]*>|\S+)/gm)) add(m.index, m[1].replace(/^<|>$/g, ""));
  for (const m of plain.matchAll(/<[a-z][^>]*?\s(?:href|src)\s*=\s*["']([^"']*)["']/gi)) add(m.index, m[1]);
  return out;
}

/** Every `href` and `src` of an HTML file, with its line. */
function htmlLinks(text) {
  const out = [];
  for (const m of text.matchAll(/\s(?:href|src)\s*=\s*["']([^"']*)["']/gi)) out.push({ line: text.slice(0, m.index).split("\n").length, target: m[1] });
  return out;
}

const problems = [];
const report = (file, line, target, why) => problems.push(`${file}:${line}: ${target} (${why})`);

/** Checks one link written in `file`. */
function check(file, { line, target }) {
  const raw = target.trim();
  if (raw === "" || SCHEME.test(raw) || raw.startsWith("//")) return;
  const hash = raw.indexOf("#");
  const path = decode(hash < 0 ? raw : raw.slice(0, hash).split("?")[0]);
  const anchor = hash < 0 ? "" : decode(raw.slice(hash + 1));
  let targetFile = file;
  if (path !== "") {
    // `/x` is the root of the site for a page of the site, else of the repository.
    const base = path.startsWith("/") ? (file.startsWith("site/") ? join(root, "site") : root) : dirname(join(root, file));
    const found = resolve(base, path.startsWith("/") ? path.slice(1) : path);
    if (relative(root, found).startsWith("..")) return report(file, line, raw, "outside the repository");
    if (!existsSync(found)) return report(file, line, raw, "no such file");
    if (statSync(found).isDirectory()) return;
    targetFile = relative(root, found).split(sep).join("/");
  }
  if (anchor === "" || anchor.startsWith("L") && /^L\d+(-L?\d+)?$/.test(anchor)) return;
  const anchors = anchorsOf(targetFile);
  if (anchors && !anchors.has(anchor) && !anchors.has(anchor.toLowerCase())) report(file, line, raw, `no heading "#${anchor}" in ${targetFile}`);
}

for (const file of markdown) for (const link of markdownLinks(readFileSync(join(root, file), "utf8"))) check(file, link);
for (const file of html) for (const link of htmlLinks(readFileSync(join(root, file), "utf8"))) check(file, link);

if (problems.length > 0) {
  console.error(problems.join("\n"));
  console.error(`\n${problems.length} broken link${problems.length === 1 ? "" : "s"} in ${markdown.length} Markdown and ${html.length} HTML files`);
  process.exit(1);
}
console.log(`links ok: ${markdown.length} Markdown and ${html.length} HTML files`);
