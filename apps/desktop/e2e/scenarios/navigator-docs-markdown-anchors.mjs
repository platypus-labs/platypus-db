// Bug NAV: the Markdown documentation (Export documentation… › Markdown) links its table of
// contents and foreign keys to anchors that no Markdown renderer generates for its headings
// when a name has an underscore (most names): the link is `#mainorderitems` but GitHub,
// GitLab and VS Code give the heading `## main.order_items` the id `mainorder_items`. The
// heading carries no explicit anchor, so every such link goes nowhere. Two names that differ
// only by punctuation (`a_b`, `ab`) also get the same anchor in both formats.
//
// Cause: `anchor()` in crates/platypus-docs/src/lib.rs keeps ASCII letters and digits only;
// markdown.rs writes `## schema.name` without an `<a id>`.
//
// Steps: SQLite with table order_items; Export documentation… of main as Markdown.
// Expected: every `(#…)` link of the file resolves to a heading's GitHub-style slug or an
// explicit anchor. Actual: `#mainorderitems` matches nothing.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { menu, row, saveConnections } from "../navigator-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "docs.db"));
  db.exec(`CREATE TABLE order_items (id INTEGER PRIMARY KEY, order_id INTEGER REFERENCES orders(id));
           CREATE TABLE orders (id INTEGER PRIMARY KEY);`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a0042"
name = "QA Docs"
driver = "sqlite"
database = '${join(dir, "docs.db")}'
save_password = false
`);
}

/** The id GitHub (and GitLab, VS Code) gives a heading. */
const slug = (heading) =>
  heading
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\- _]/gu, "")
    .replace(/ /g, "-");

export default async function (page, { dir }) {
  await page.eval(`${row("QA Docs")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("order_items")} !== undefined`, 15_000);

  const docs = `document.querySelector('.modal.docs-dialog')`;
  const button = (text) => `[...${docs}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
  const out = join(dir, "docs.md");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(out)})`);
  await menu(page, "main", "Export documentation…");
  await page.waitFor(`${docs} !== null`, 15_000);
  await page.eval(`(() => { const s = ${docs}.querySelector('.docs-format select'); s.value = 'markdown'; s.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  await page.eval(`${button("Choose file…")}.click()`);
  await page.waitFor(`${docs}.querySelector('input[aria-label="File"]').value === ${JSON.stringify(out)}`);
  await page.eval(`${button("Export")}.click()`);
  await page.waitFor(`${docs}.textContent.includes('Documentation written')`, 30_000);

  const md = readFileSync(out, "utf8");
  const targets = new Set([
    ...[...md.matchAll(/^#{1,6} (.+)$/gm)].map((m) => slug(m[1])),
    ...[...md.matchAll(/<a (?:id|name)="([^"]+)"/g)].map((m) => m[1]),
  ]);
  const links = [...new Set([...md.matchAll(/\]\(#([^)]+)\)/g)].map((m) => m[1]))];
  assert.ok(links.length > 0, "the documentation links its tables");
  const broken = links.filter((l) => !targets.has(l));
  assert.deepEqual(broken, [], `links without a target; headings: ${[...targets].join(", ")}`);
}
