// MongoDB from the `mongo` profile of docker/compose.yml: the navigator lists databases and
// collections, the data viewer writes mongosh (filter document, sort), the document panel
// shows the focused row's nested fields, the editor runs mongosh statements, and fake data
// fills a collection with an array and a document field.
// Skipped when MongoDB is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { revealItem } from "../menu-sub.mjs";

const NAME = "E2E Mongo";
const PORT = 57017;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `${visibleTab}?.querySelector('.statusbar span')?.textContent ?? ''`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function skip() {
  return new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port: PORT, timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolve(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done(`no MongoDB on port ${PORT} (docker compose -f docker/compose.yml --profile mongo up -d)`));
    socket.on("timeout", () => done(`no MongoDB on port ${PORT}`));
  });
}

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000027"
name = "${NAME}"
driver = "mongo"
host = "localhost"
port = ${PORT}
database = "platypus?authSource=admin"
user = "platypus"
ssl_mode = "disable"
save_password = false
`,
  );
}

/** Clears the status, so a wait cannot match the previous result. */
const clearStatus = (page) => page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);

/** Waits until no statement runs (Cancel is disabled) and the status passes `test`. */
async function waitStatus(page, test) {
  const idle = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')?.disabled`;
  await page.waitFor(`(() => { const s = ${status}; return ${idle} && s !== '' && !s.includes('Executing') && (${test}); })()`, 20_000);
  return page.eval(status);
}

async function menu(page, selector) {
  const r = await page.rect(selector);
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  return page.eval(`[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`);
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);

  // The current database opens by itself, with its collections (views in their folder).
  await page.waitFor(`${row("customers")} !== undefined && ${row("Views")} !== undefined`, 20_000);
  await page.eval(`${row("customers")}.id = 'e2e-customers'`);
  const items = await menu(page, "#e2e-customers");
  for (const item of ["Open data", "Generate SQL", "Drop…"]) assert.ok(items.includes(item), `${item} in ${items}`);
  assert.ok(!items.includes("Visual query builder"), `no SQL builder in ${items}`);
  await revealItem(page, "Open data");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent === 'Open data').click()`);

  // The data viewer runs mongosh and shows the document of the focused row.
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`);
  assert.match(await waitStatus(page, "s.includes('row')"), /rows/);
  assert.equal(await page.eval(editorText), "db.customers.find({})");
  const grid = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  await page.click(grid.x + 80, grid.y + 40); // a cell of the first row
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.document-panel .doc-key')].some(k => k.textContent === 'address')`, 10_000);

  // A header click sorts with .sort().
  await clearStatus(page);
  await page.click(grid.x + 80, grid.y + 14);
  await waitStatus(page, "true");
  assert.match(await page.eval(editorText), /\.sort\(\{ _id: 1 \}\)$/);

  // The editor runs a mongosh statement.
  assert.equal(await run(page, "db.orders.countDocuments({ status: 'paid' })", "s === '1 row'"), "1 row");
  await countIs(page, 5000);

  await generateData(page);
}

/** Runs `statement` in the editor; returns the status once it ends and passes `test`. */
async function run(page, statement, test = "true") {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(statement);
  await page.waitFor(`${editorText} === ${JSON.stringify(statement)}`);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  return waitStatus(page, test);
}

/** Waits for the count a `countDocuments` returned, read in the document panel (the grid draws its cells). */
async function countIs(page, n) {
  const grid = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  await page.click(grid.x + 80, grid.y + 40);
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.document-panel .doc-number')].some(n => n.textContent === '${n}')`, 10_000);
}

/** Fake documents for a collection with an array and a document field: generators of their types, inserted with insertMany. */
async function generateData(page) {
  await run(page, "db.fake_e2e.drop()");
  await run(page, `db.fake_e2e.insertOne({ _id: 1, name: "seed", tags: ["a"], address: { city: "Roma" } })`);
  await page.eval(`${row("platypus")}.id = 'e2e-db'`);
  await menu(page, "#e2e-db");
  await revealItem(page, "Refresh");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Refresh')).click()`);
  await page.waitFor(`${row("fake_e2e")} !== undefined`, 15_000);
  await page.eval(`${row("fake_e2e")}.id = 'e2e-fake'`);
  await menu(page, "#e2e-fake");
  await revealItem(page, "Generate data");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Generate data')).click()`);

  const gen = `document.querySelector('.modal.generate-data')`;
  await page.waitFor(`${gen}?.querySelectorAll('select').length === 4`, 15_000);
  assert.deepEqual(await page.eval(`[...${gen}.querySelectorAll('select')].map(s => s.value)`), ["next_key", "full_name", "word_list", "document"]);
  await page.eval(`(() => {
    const el = ${gen}.querySelector('input[type=number]');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, '20');
    el.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  const button = (text) => `[...${gen}.querySelectorAll('button')].find(b => b.textContent.trim() === '${text}')`;
  await page.eval(`${button("Generate")}.click()`);
  await page.waitFor(`${gen}.textContent.includes('Added 20 rows') || ${gen}.textContent.includes('Nothing was added')`, 20_000);
  assert.ok((await page.eval(`${gen}.textContent`)).includes("Added 20 rows"), await page.eval(`${gen}.textContent`));
  await page.eval(`${button("Close")}.click()`);

  // The new documents follow the largest _id, with an array and a document.
  await run(page, "db.fake_e2e.countDocuments({ _id: { $gt: 1, $lte: 21 }, tags: { $type: 'array' }, address: { $type: 'object' } })", "s === '1 row'");
  await countIs(page, 20);
  await run(page, "db.fake_e2e.drop()");
}
