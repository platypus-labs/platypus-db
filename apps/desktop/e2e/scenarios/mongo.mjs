// MongoDB from the `mongo` profile of docker/compose.yml: the navigator lists databases and
// collections, the data viewer writes mongosh (filter document, sort), the document panel
// shows the focused row's nested fields, and the editor runs mongosh statements.
// Skipped when MongoDB is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

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
  for (const item of ["Open data", "Generate SELECT", "Drop…"]) assert.ok(items.includes(item), `${item} in ${items}`);
  assert.ok(!items.includes("Visual query builder"), `no SQL builder in ${items}`);
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
  const statement = "db.orders.countDocuments({ status: 'paid' })";
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(statement);
  await page.waitFor(`${editorText} === ${JSON.stringify(statement)}`);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  assert.equal(await waitStatus(page, "s.includes('row')"), "1 row");
  // The grid draws its cells: read the count in the document panel.
  const counted = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  await page.click(counted.x + 80, counted.y + 40);
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.document-panel .doc-number')].some(n => n.textContent === '5000')`, 10_000);
}
