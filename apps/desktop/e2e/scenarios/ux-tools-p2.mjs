// UX review, second pass, on the tools (SQLite):
// - the query builder: the list's tables take the keyboard (focus, Enter) and a "+" button; a
//   joined table leaves room (80 px) and the join says on what columns it is, in a button that
//   changes it (JOIN / LEFT JOIN); the canvas has no scroll bar when nothing overflows;
// - the notebook: hints in the empty text and SQL cells, a short label for "on the results",
//   a cell with content asks before it goes (Cancel keeps it), an empty one goes at once;
// - the database picker has a close button; the connection dialog keeps the long labels short
//   with the explanation below, and the two commands are one under the other.
import assert from "node:assert/strict";
import { expand, menu } from "../transfer-helpers.mjs";
import { sqliteConnection } from "../query.mjs";
import { treeMenu } from "../grid-helpers.mjs";

const NAME = "UX tools db";
const builder = `document.querySelector('.builder-tab:not([style*="none"])')`;
const builderSql = `(${builder}?.querySelector('.builder-sql')?.textContent ?? '')`;
const source = (name) => `[...${builder}.querySelectorAll('.builder-source')].find(r => r.textContent === ${JSON.stringify(name)})`;
const box = (name) => `${builder}.querySelector('.builder-box[data-table=${JSON.stringify(name)}]')`;
const canvas = `${builder}.querySelector('.builder-canvas')`;
const tab = `[...document.querySelectorAll('.notebook-tab')].find(t => t.style.display !== 'none')`;
const cell = (name) => `${tab}?.querySelector('[data-name=${JSON.stringify(name)}]')`;
const click = (scope, action) => `${scope}.querySelector('[data-action=${JSON.stringify(action)}]').click()`;
const cellCount = `${tab}.querySelectorAll('.notebook-cell').length`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "ux-tools.db",
    id: "00000000-0000-4000-8000-000000000b83",
    name: NAME,
    sql: `CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
      CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL);
      CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);`,
  });
}

async function queryBuilder(page) {
  await expand(page, NAME, "customers");
  await menu(page, "customers", "Visual query builder", null, NAME);
  await page.waitFor(`${builderSql}.includes('FROM customers c')`, 15_000);
  await page.waitFor(`${box("customers")} !== null`, 10_000);
  // One box fits: the canvas has nothing to scroll.
  assert.equal(await page.eval(`${canvas}.scrollWidth <= ${canvas}.clientWidth`), true, "no horizontal scroll bar");

  // The rows of the list take the keyboard.
  assert.equal(await page.eval(`${source("orders")}.tabIndex`), 0);
  await page.eval(`${source("orders")}.focus()`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${builderSql}.includes('JOIN orders o ON o.customer_id = c.id')`, 10_000);
  // The new box is apart from the first, with room for the join between them.
  const gap = await page.eval(`${box("orders")}.getBoundingClientRect().left - ${box("customers")}.getBoundingClientRect().right`);
  assert.ok(gap >= 60, `room for the join: ${gap}`);
  // The join says on what columns it is, and its button changes it.
  assert.equal(await page.eval(`${builder}.querySelector('.builder-join-on').textContent`), "o.customer_id = c.id");
  assert.equal(await page.eval(`${builder}.querySelector('.builder-join').textContent`), "JOIN");
  await page.eval(`${builder}.querySelector('.builder-join').click()`);
  await page.waitFor(`${builderSql}.includes('LEFT JOIN orders o')`);
  assert.equal(await page.eval(`${builder}.querySelector('.builder-join').textContent`), "LEFT JOIN");
  assert.equal(await page.eval(`${builder}.querySelector('.builder-join-on').textContent`), "o.customer_id = c.id");

  // The "+" of a row adds it too; a table with no key to the others is cross joined.
  assert.equal(await page.eval(`${source("notes")}.querySelector('.builder-add').getAttribute('aria-label')`), "Add notes to the canvas");
  await page.eval(`${source("notes")}.querySelector('.builder-add').click()`);
  await page.waitFor(`${builderSql}.includes('CROSS JOIN notes')`, 10_000);
  // Enter on a row of a table already there adds another box (an alias of its own), never an error.
  assert.equal(await page.eval(`document.querySelectorAll('.notice.error').length`), 0);
  // "Run" says what it does.
  assert.match(await page.eval(`${builder}.querySelector('.actions .primary').title`), /new editor tab/);
}

async function notebook(page) {
  const before = await page.eval(`document.querySelectorAll('.notebook-tab').length`);
  await treeMenu(page, NAME, "New notebook");
  await page.waitFor(`document.querySelectorAll('.notebook-tab').length > ${before} && ${cell("c1")} != null`);
  // A new notebook is one SQL cell; the text cell is added by "+ Text".
  assert.equal(await page.eval(cellCount), 1);
  await page.eval(click(tab, "add_text"));
  await page.waitFor(`${tab}.querySelector('textarea.notebook-text') !== null`);
  // The empty cells say how they are written.
  assert.match(await page.eval(`${tab}.querySelector('textarea.notebook-text').placeholder`), /Markdown/);
  await page.eval(click(`${tab}.querySelector('.notebook-cell.markdown')`, "delete"));
  await page.waitFor(`${cellCount} === 1`);
  await page.waitFor(`${cell("c1")}.querySelector('.cm-placeholder') !== null`);
  assert.match(await page.eval(`${cell("c1")}.querySelector('.cm-placeholder').textContent`), /Ctrl\+Enter/);
  // The first cell has no cells before it: no "on the results" box.
  assert.equal(await page.eval(`${cell("c1")}.querySelector('label.check')`), null);

  // An empty cell goes at once: no question. The next one has a short label, not the sentence.
  await page.eval(click(tab, "add_sql"));
  await page.waitFor(`${cellCount} === 2`);
  assert.equal(await page.eval(`${cell("c2")}.querySelector('label.check').textContent.trim()`), "On results");
  await page.eval(click(cell("c2"), "delete"));
  await page.waitFor(`${cellCount} === 1`);
  assert.equal(await page.eval(`document.querySelector('.modal.confirm')`), null);

  // One with a text asks; Cancel keeps it.
  await page.eval(`${cell("c1")}.querySelector('.cm-content').focus()`);
  await page.type("SELECT 1");
  await page.waitFor(`${cell("c1")}.querySelector('.cm-line').textContent === 'SELECT 1'`);
  await page.key("Escape", "Escape", 27);
  await page.eval(click(cell("c1"), "delete"));
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') === null`);
  assert.equal(await page.eval(cellCount), 1, "Cancel keeps the cell");
  // Deleting it takes the text with it (a second cell keeps the notebook from being empty).
  await page.eval(click(tab, "add_sql"));
  await page.waitFor(`${cellCount} === 2`);
  await page.eval(click(cell("c1"), "delete"));
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await page.waitFor(`${cellCount} === 1`);
}

async function pickerAndConnection(page) {
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`, 10_000);
  // A way out at the top, where it shows even in a short window.
  assert.ok(await page.eval(`document.querySelector('.modal.driver-picker .modal-head .modal-close') !== null`));
  await page.eval(`document.querySelector('.modal.driver-picker .modal-head .modal-close').click()`);
  await page.waitFor(`document.querySelector('.modal.driver-picker') === null`);

  // The form of a server database: PostgreSQL is the first match of the picker.
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`, 10_000);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.connection-dialog .dialog-tabs') !== null`, 10_000);
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network')).click()`);
  await page.waitFor(`document.querySelector('[data-page="network"]').hidden === false`);
  const labels = `[...document.querySelectorAll('[data-page="network"] label.field > span:first-child')].map(s => s.textContent)`;
  // Short labels; the explanation is a line under the field.
  assert.ok((await page.eval(labels)).includes("Client certificate (PEM)"), JSON.stringify(await page.eval(labels)));
  assert.match(
    await page.eval(`document.querySelector('[data-field="ssl_client_cert"]').closest('label').querySelector('small').textContent`),
    /authenticate clients by certificate/,
  );
  // The two commands are one under the other, as wide as each other.
  const rects = await page.eval(`['before_connect', 'after_disconnect'].map(f => { const r = document.querySelector('[data-field=' + f + ']').getBoundingClientRect(); return { y: r.y, w: Math.round(r.width) }; })`);
  assert.ok(rects[1].y > rects[0].y + 10, `one under the other: ${JSON.stringify(rects)}`);
  assert.equal(rects[0].w, rects[1].w);
  // The first thing the dialog focuses is not its close button.
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.connection-dialog') === null`);
}

export default async function (page) {
  await queryBuilder(page);
  await notebook(page);
  await pickerAndConnection(page);
}
