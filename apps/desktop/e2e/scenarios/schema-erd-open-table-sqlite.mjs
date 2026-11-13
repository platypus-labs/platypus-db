// SCH-13 (medium). A double click on a table of the ER diagram does not open it, although the
// box handles onDoubleClick: the pointerdown that starts a drag captures the pointer on the
// <svg>, so the clicks and the dblclick go to the svg, not to the table's <g>. Dragging works.
// Steps: an ER diagram of main; double click the table customers.
// Expected: the table opens in a tab. Actual: nothing.
// Suspected: apps/desktop/src/erd/ErdTab.tsx:147-152 (setPointerCapture on every pointerdown;
// capture only once the pointer moves, or handle dblclick on the svg by hit-testing).
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, openPath, rowMenu, saveEngines, visibleErd } from "../schema-compare-helpers.mjs";

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, `CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));`);
  ENGINES.sqlite.seed(dir, DST, "");
  saveEngines(dir, ["sqlite"]);
}

const box = (name) => `[...${visibleErd}.querySelectorAll('.erd-table')].find(g => g.querySelector('text').textContent === ${JSON.stringify(name)})`;

export default async function (page) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", "ER diagram");
  await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === 2`, 20_000);

  // Dragging works: the table moves by what the mouse moved.
  const before = await page.eval(`${box("orders")}.getAttribute('transform')`);
  await page.eval(`${box("orders")}.querySelector('rect').id = 'qa3-orders'`);
  const r = await page.rect("#qa3-orders");
  await page.mouse("mousePressed", r.x + 20, r.y + 10);
  await page.mouse("mouseMoved", r.x + 60, r.y + 50, { buttons: 1 });
  await page.mouse("mouseMoved", r.x + 120, r.y + 90, { buttons: 1 });
  await page.mouse("mouseReleased", r.x + 120, r.y + 90);
  await page.waitFor(`${box("orders")}.getAttribute('transform') !== ${JSON.stringify(before)}`, 5000);

  await page.eval(`${box("customers")}.querySelector('rect').id = 'qa3-customers'`);
  await page.clickSel("#qa3-customers");
  await page.clickSel("#qa3-customers", { clickCount: 2 });
  await page.sleep(1500);
  const tabs = await page.eval(`[...document.querySelectorAll('.tab')].map(t => t.textContent)`);
  assert.ok(tabs.some((t) => t.startsWith("customers")), `no tab for customers after a double click: ${JSON.stringify(tabs)}`);
}
