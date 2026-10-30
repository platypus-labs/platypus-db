// The table editor from the object tab and from the data viewer: "Edit table…" next to "Open
// data" in a table's tab, and "Structure" / "Edit table…" in the data viewer's bar. Each brings
// forward the tab already open on the table instead of opening another one; a view has its
// structure but no table editor.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { menu, row, saveConnections } from "../qa-nav-helpers.mjs";
import { editor } from "../qa-nav-table-editor.mjs";

const NAME = "Fix Lite";
const objectTab = `[...document.querySelectorAll('.object-tab:not(.table-editor)')].find(t => t.style.display !== 'none')`;
const dataTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
/** The button `label` of `scope` (a visible tab's header or bar), or undefined. */
const button = (scope, label) => `[...(${scope}?.querySelectorAll('button') ?? [])].find(b => b.textContent === ${JSON.stringify(label)})`;
const header = `${objectTab}?.querySelector('.object-header')`;
const viewerBar = `${dataTab}?.querySelector('.viewer-bar')`;
/** The titles of the open tabs. */
const tabTitles = `[...document.querySelectorAll('.tab')].map(t => t.textContent)`;
const count = (title) => `${tabTitles}.filter(t => t.startsWith(${JSON.stringify(title)})).length`;
const editorTabs = `document.querySelectorAll('.table-editor').length`;
const objectTabs = `document.querySelectorAll('.object-tab:not(.table-editor)').length`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "edit-table.db"));
  db.exec(`CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL, name TEXT);
           CREATE VIEW v_customers AS SELECT id, email FROM customers;
           INSERT INTO customers (email, name) VALUES ('a@example.com', 'Ann'), ('b@example.com', 'Bob');`);
  db.close();
  saveConnections(
    dir,
    `[[connections]]
id = "00000000-0000-4000-8000-0000000f0001"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "edit-table.db")}'
save_password = false
`,
  );
}

/** Clicks `expression` (a button) with the mouse. */
async function click(page, expression) {
  await page.eval(`(${expression}).id = 'e2e-button'`);
  await page.clickSel("#e2e-button");
  await page.eval(`document.getElementById('e2e-button')?.removeAttribute('id')`);
}

/** Double-clicks the navigator row `label`: tables and views open their data. */
async function openData(page, label) {
  await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
}

/** The key of the active tab, to show it again with `showTab`. */
const activeKey = (page) => page.eval(`document.querySelector('.tab.active')?.dataset.tab`);

/** Makes the tab of `key` active. */
async function showTab(page, key) {
  await click(page, `document.querySelector('.tab[data-tab="${key}"]')`);
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("customers")} !== undefined && ${row("v_customers")} !== undefined`, 15_000);

  // The table's tab: "Edit table…" beside "Open data", opening the editor on its columns.
  await menu(page, "customers", "Open details");
  await page.waitFor(`${button(header, "Open data")} !== undefined`, 10_000);
  const details = await activeKey(page);
  assert.ok(await page.eval(`${button(header, "Edit table…")} !== undefined`), "Edit table… in the table's tab");
  await click(page, button(header, "Edit table…"));
  await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 3`, 10_000);
  assert.equal(await page.eval(editorTabs), 1);

  // Again from the table's tab: the editor already open comes forward, no second one.
  await showTab(page, details);
  await page.waitFor(`${objectTab} !== undefined`);
  await click(page, button(header, "Edit table…"));
  await page.waitFor(`${editor} !== undefined`, 5000);
  assert.equal(await page.eval(editorTabs), 1, "the open editor comes forward");

  // A view has its data and its DDL, but no table editor.
  await menu(page, "v_customers", "Open details");
  await page.waitFor(`${objectTab}?.querySelector('.object-title strong')?.textContent === 'v_customers'`, 10_000);
  await page.waitFor(`${button(header, "Open data")} !== undefined`, 10_000);
  assert.equal(await page.eval(`${button(header, "Edit table…")} === undefined`), true, "no table editor for a view");

  // The data viewer of the table: Structure goes back to the table's tab, Edit table… to the editor.
  await openData(page, "customers");
  await page.waitFor(`${button(viewerBar, "Structure")} !== undefined`, 15_000);
  await page.waitFor(`${button(viewerBar, "Edit table…")} !== undefined`);
  const data = await activeKey(page);
  const before = await page.eval(objectTabs);
  await click(page, button(viewerBar, "Structure"));
  await page.waitFor(`${objectTab}?.querySelector('.object-title strong')?.textContent === 'customers'`, 10_000);
  assert.equal(await activeKey(page), details, "the table's tab already open comes forward");
  assert.equal(await page.eval(objectTabs), before);

  await showTab(page, data);
  await page.waitFor(`${viewerBar} != null`, 10_000);
  await click(page, button(viewerBar, "Edit table…"));
  await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 3`, 10_000);
  assert.equal(await page.eval(editorTabs), 1, "still one table editor");

  // Open data in the table's tab goes back to the data viewer already open.
  await showTab(page, details);
  await page.waitFor(`${button(header, "Open data")} !== undefined`);
  const queryTabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await click(page, button(header, "Open data"));
  await page.waitFor(`document.querySelector('.tab.active')?.dataset.tab === ${JSON.stringify(data)}`, 5000);
  assert.equal(await page.eval(`document.querySelectorAll('.query-tab').length`), queryTabs, "no second data viewer");

  // A data viewer made read-only does not open the editor.
  await click(page, `[...${dataTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Read-only')`);
  await page.waitFor(`${button(viewerBar, "Edit table…")}?.disabled === true`, 5000);
  await click(page, `[...${dataTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Read-only')`);
  await page.waitFor(`${button(viewerBar, "Edit table…")}?.disabled === false`, 5000);

  // The view's data viewer: its structure, no editor.
  await openData(page, "v_customers");
  await page.waitFor(`${button(viewerBar, "Structure")} !== undefined`, 15_000);
  assert.equal(await page.eval(`${button(viewerBar, "Edit table…")} === undefined`), true, "no table editor for a view's data");
  await click(page, button(viewerBar, "Structure"));
  await page.waitFor(`${objectTab}?.querySelector('.object-title strong')?.textContent === 'v_customers'`, 10_000);
  assert.equal(await page.eval(count("v_customers")), 2, "the view's data and its details, no third tab");
}
