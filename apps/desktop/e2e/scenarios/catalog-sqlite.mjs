// Data catalog: a note and a tag on a table, the mark in the navigator, the #tag filter, the
// column that describes a referenced table's rows, and both still there after the page is loaded
// again (they live in catalog.db, not in the page). Then the schema documentation, exported as
// Markdown with the note and the ER diagram.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "Catalog db";
const ID = "00000000-0000-4000-8000-000000000018";
const NOTE = "One row per order";
const rows = (label) =>
  `[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const row = (label) => `${rows(label)}[0]`;
const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;
const noteField = `${objectTab}?.querySelector('textarea[aria-label="Note"]')`;
const rowLabel = `${objectTab}?.querySelector('.row-label select')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "catalog.sqlite"));
  db.exec(`CREATE TABLE orders (id INTEGER PRIMARY KEY, email TEXT);
           CREATE TABLE items (id INTEGER PRIMARY KEY);
           CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, company TEXT);
           CREATE TABLE invoices (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "${ID}"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "catalog.sqlite")}'
save_password = false
`,
  );
}

async function expand(page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${row("orders")} !== undefined && ${row("items")} !== undefined`, 15_000);
}

async function openDetails(page, table = "orders") {
  await page.waitFor(`${row(table)} !== undefined`, 15_000);
  await page.eval(`${row(table)}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.eval(`document.getElementById('e2e-target').removeAttribute('id')`);
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === 'Open details').click()`,
  );
  // Another object tab may be showing: wait for this table's.
  await page.waitFor(`${objectTab}?.querySelector('.object-title strong')?.textContent === ${JSON.stringify(table)} && ${noteField} != null`, 15_000);
}

/** Opens the menu of the tree row `label` and clicks `item`. */
async function menu(page, label, item) {
  await page.waitFor(`${row(label)} !== undefined`, 15_000);
  await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === ${JSON.stringify(item)}).click()`,
  );
}

/** The schema documentation as Markdown, in the file the save picker answers: the note and
 *  the diagram are in it. The picker answers an .html file, which the switch to Markdown renames:
 *  the new name shows that the dialog took the format. */
async function exportDocs(page, dir) {
  const docs = `document.querySelector('.modal.docs-dialog')`;
  const button = (text) => `[...${docs}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
  const file = `${docs}.querySelector('input[aria-label="File"]').value`;
  const picked = join(dir, "docs.html");
  const out = join(dir, "docs.md");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(picked)})`);
  await menu(page, "main", "Export documentation…");
  await page.waitFor(`${docs} !== null`, 15_000);
  await page.eval(`${button("Choose file…")}.click()`);
  await page.waitFor(`${file} === ${JSON.stringify(picked)}`);
  await page.eval(
    `(() => { const s = ${docs}.querySelector('.docs-format select'); s.value = 'markdown'; s.dispatchEvent(new Event('change', { bubbles: true })); })()`,
  );
  await page.waitFor(`${file} === ${JSON.stringify(out)}`);
  await page.waitFor(`!${button("Export")}.disabled`);
  await page.eval(`${button("Export")}.click()`);
  await page.waitFor(`${docs}.textContent.includes('Documentation written')`, 30_000);
  const text = readFileSync(out, "utf8");
  assert.doesNotMatch(text, /<!doctype/i);
  // Each table has an explicit anchor, its name as written.
  assert.match(text, /^\| \[main\.orders\]\(#main\.orders\) \| table \|/m);
  assert.match(text, /^<a id="main\.orders"><\/a>$/m);
  assert.match(text, /One row per order/);
  assert.match(text, /```mermaid\nerDiagram\n/);
  await page.eval(`${button("Close")}.click()`);
}

export default async function (page, { dir }) {
  await expand(page);
  // Nothing marked yet: the wait below is on a change.
  assert.equal(await page.eval(`${row("orders")}.querySelector('.catalog-mark') === null`), true);
  await openDetails(page);
  assert.equal(await page.eval(`${noteField}.value`), "");

  await page.eval(`${noteField}.focus()`);
  await page.type(NOTE);
  await page.waitFor(`${noteField}.value === ${JSON.stringify(NOTE)}`);
  // Leaving the field saves the note.
  await page.eval(`${objectTab}.querySelector('.tag-input input').focus()`);
  await page.waitFor(`${row("orders")}.querySelector('.catalog-mark') !== null`);

  await page.type("pii");
  await page.waitFor(`${objectTab}.querySelector('.tag-input input').value === 'pii'`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`[...${objectTab}.querySelectorAll('.tag-chip')].some(c => c.textContent.startsWith('#pii'))`);

  // The filter keeps only what carries the tag.
  assert.equal(await page.eval(`${rows("items")}.length`), 1);
  await page.eval(`document.querySelector('.tree-filter').focus()`);
  await page.type("#pii");
  await page.waitFor(`document.querySelector('.tree-filter').value === '#pii'`);
  await page.waitFor(`${rows("items")}.length === 0 && ${rows("orders")}.length > 0`);
  // Escape empties the filter, so customers is listed again.
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.tree-filter').value === '' && ${rows("customers")}.length > 0`);

  // The row label of customers, for the foreign key invoices.customer_id: Automatic until picked.
  await openDetails(page, "customers");
  // The columns come with the table's details.
  await page.waitFor(`[...(${rowLabel}?.options ?? [])].some(o => o.value === 'company')`, 15_000);
  assert.equal(await page.eval(`${rowLabel}.value`), "");
  await page.eval(`(() => { const s = ${rowLabel}; s.value = 'company'; s.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  // Wait on catalog.db, not on the select the line above has just set.
  await page.waitFor(
    `window.__TAURI_INTERNALS__.invoke('catalog_display_columns', { connectionId: '${ID}' }).then(all => all.some(d => d.key.object === 'customers' && d.column === 'company'))`,
  );
  await page.waitFor(`${rowLabel}.value === 'company'`);

  // A new load of the page: the note and the row label come from catalog.db.
  // The marker only exists on the old document: its absence means the new one has loaded.
  await page.eval(`window.__e2eBeforeReload = true; location.reload()`);
  await page.waitFor(`window.__e2eBeforeReload === undefined && document.querySelector('.shell') !== null && ${row(NAME)} !== undefined`, 30_000);
  // The filter is component state: it starts empty, so every table is listed.
  assert.equal(await page.eval(`document.querySelector('.tree-filter').value`), "");
  await expand(page);
  await page.waitFor(`${row("orders")}.querySelector('.catalog-mark') !== null`);
  await openDetails(page);
  await page.waitFor(`${noteField}.value === ${JSON.stringify(NOTE)}`);
  assert.equal(await page.eval(`[...${objectTab}.querySelectorAll('.tag-chip')].some(c => c.textContent.startsWith('#pii'))`), true);
  // The new select starts on Automatic and shows the pick once the catalog has loaded.
  await openDetails(page, "customers");
  await page.waitFor(`${rowLabel}?.value === 'company'`, 15_000);

  await exportDocs(page, dir);
}
