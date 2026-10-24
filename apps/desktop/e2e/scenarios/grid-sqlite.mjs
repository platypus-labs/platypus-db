// Read-only grid features on a throwaway SQLite database: copy with headers and as JSON,
// search with Ctrl+F, the value panel (F7) and a quick filter from the menu.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";

const NAME = "E2E grid";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "grid.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, doc TEXT);
           INSERT INTO items VALUES (1, 'apple', '{"a":1,"b":[2,3]}'), (2, 'banana', NULL), (3, 'cherry', 'x');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000003"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

async function copied(page, run) {
  await page.eval(`window.__copied = undefined`);
  await run();
  await page.waitFor(`window.__copied !== undefined`);
  return page.eval(`window.__copied`);
}

/** Right-clicks the first row, `x` pixels from the grid's left edge. */
async function openMenu(page, x) {
  const r = await page.rect(`#e2e-grid`);
  await page.click(r.x + x, r.y + 52, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  // The current schema opens by itself.
  await page.eval(`${treeRow(NAME)}.click()`);
  await page.waitFor(`${treeRow("items")} !== undefined`);
  await page.eval(`${treeRow("items")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${status} === '3 rows'`, 15_000);

  // The grid draws in the bundled monospace font and, once it has loaded, sizes the columns
  // with it rather than with the fallback it may have measured first.
  const canvas = `${visibleTab}.querySelector('.grid-canvas')`;
  assert.match(await page.eval(`${canvas}.dataset.font`), /^12px "JetBrains Mono"/);
  await page.waitFor(`${canvas}.dataset.measuredWith?.includes('JetBrains Mono')`);

  await page.eval(`${grid}.id = 'e2e-grid'`);
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);

  // Copy with headers keeps line breaks out of the way and full values.
  await page.eval(`${grid}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  const tsv = await copied(page, () => page.key("C", "KeyC", 67, CTRL | SHIFT));
  assert.equal(tsv, 'id\tname\tdoc\n1\tapple\t"{""a"":1,""b"":[2,3]}"\n2\tbanana\t\n3\tcherry\tx');

  // Copy as JSON, from the submenu.
  await openMenu(page, 40);
  await page.eval(`${menuButton("Copy as")}.click()`);
  await page.waitFor(`${menuButton("JSON")} !== undefined`);
  const json = await copied(page, () => page.eval(`${menuButton("JSON")}.click()`));
  assert.deepEqual(JSON.parse(json), [
    { id: 1, name: "apple", doc: '{"a":1,"b":[2,3]}' },
    { id: 2, name: "banana", doc: null },
    { id: 3, name: "cherry", doc: "x" },
  ]);

  // Ctrl+F finds the loaded cells.
  await page.eval(`${grid}.focus()`);
  await page.key("f", "KeyF", 70, CTRL);
  await page.waitFor(`document.querySelector('.grid-search input') !== null`);
  await page.type("an");
  await page.waitFor(`document.querySelector('.grid-search span')?.textContent === '1 of 1'`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.grid-search') === null`);

  // The value panel shows the focused cell, JSON formatted.
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, CTRL);
  await page.key("F7", "F7", 118);
  await page.waitFor(`document.querySelector('.value-panel') !== null`);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.waitFor(`document.querySelector('.value-text')?.textContent.includes('"b": [')`);
  await page.key("F7", "F7", 118);
  await page.waitFor(`document.querySelector('.value-panel') === null`);

  // Quick filter on the focused value.
  await openMenu(page, 130); // the name column
  await page.eval(`${menuButton("Filter")}.click()`);
  await page.waitFor(`${menuButton("Equal to this value")} !== undefined`);
  await page.eval(`${menuButton("Equal to this value")}.click()`);
  await page.waitFor(`${status} === '1 row'`, 15_000);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.viewer-bar input').value`), "name = 'apple'");
}
