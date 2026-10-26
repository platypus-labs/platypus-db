// Data catalog: a note and a tag on a table, the mark in the navigator, the #tag filter, and the
// note still there after the page is loaded again (it lives in catalog.db, not in the page).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "Catalog db";
const NOTE = "One row per order";
const rows = (label) =>
  `[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const row = (label) => `${rows(label)}[0]`;
const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;
const noteField = `${objectTab}?.querySelector('textarea[aria-label="Note"]')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "catalog.sqlite"));
  db.exec(`CREATE TABLE orders (id INTEGER PRIMARY KEY, email TEXT);
           CREATE TABLE items (id INTEGER PRIMARY KEY);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000018"
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

async function openDetails(page) {
  await page.eval(`${row("orders")}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.eval(`document.getElementById('e2e-target').removeAttribute('id')`);
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === 'Open details').click()`,
  );
  await page.waitFor(`${noteField} != null`, 15_000);
}

export default async function (page) {
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

  // A new load of the page: the note comes from catalog.db.
  // The marker only exists on the old document: its absence means the new one has loaded.
  await page.eval(`window.__e2eBeforeReload = true; location.reload()`);
  await page.waitFor(`window.__e2eBeforeReload === undefined && document.querySelector('.shell') !== null && ${row(NAME)} !== undefined`, 30_000);
  // The filter is component state: it is empty again, so both tables are listed.
  assert.equal(await page.eval(`document.querySelector('.tree-filter').value`), "");
  await expand(page);
  await page.waitFor(`${row("orders")}.querySelector('.catalog-mark') !== null`);
  await openDetails(page);
  await page.waitFor(`${noteField}.value === ${JSON.stringify(NOTE)}`);
  assert.equal(await page.eval(`[...${objectTab}.querySelectorAll('.tag-chip')].some(c => c.textContent.startsWith('#pii'))`), true);
}
