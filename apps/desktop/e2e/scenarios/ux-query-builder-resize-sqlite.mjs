// The visual query builder can be resized on SQLite: the tables list by its handle (dragging
// widens it, an excess is stopped at the largest and the smallest width, a double click goes
// back to the default, and a builder opened later has the width left by the last one), and a
// table box by its corner grip (wider and taller, never below the minimum, and the SQL does not
// change).
import assert from "node:assert/strict";
import { expand, menu } from "../transfer-helpers.mjs";
import { sqliteConnection } from "../query.mjs";

const NAME = "Builder resize db";
const builder = `document.querySelector('.builder-tab:not([style*="none"])')`;
const side = `${builder}.querySelector('.builder-tables')`;
const handle = `${builder}.querySelector('.builder-tables + .panel-resizer')`;
const sideWidth = `Math.round(${side}.getBoundingClientRect().width)`;
const builderSql = `(${builder}?.querySelector('.builder-sql')?.textContent ?? '')`;
const box = `${builder}.querySelector('.builder-box[data-table="customers"]')`;
const boxSize = `(() => { const r = ${box}.getBoundingClientRect(); return { w: Math.round(r.width), h: Math.round(r.height) }; })()`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "builder-resize.db",
    id: "00000000-0000-4000-8000-000000000b82",
    name: NAME,
    sql: `CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, email TEXT, city TEXT, country TEXT, note TEXT);
      CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL);`,
  });
}

/** Drags the centre of `target` (an expression for an element) by (dx, dy). */
async function dragBy(page, target, dx, dy) {
  const r = await page.rectOf(target);
  const x = r.x + r.w / 2;
  const y = r.y + r.h / 2;
  await page.drag(x, y, x + dx, y + dy);
}

export default async function (page) {
  await expand(page, NAME, "customers");
  await menu(page, "customers", "Visual query builder", null, NAME);
  await page.waitFor(`${builderSql}.includes('FROM customers c')`, 15_000);
  const start = await page.eval(sideWidth);
  assert.equal(start, 220, "the default width");

  // Dragging the handle widens the list.
  await dragBy(page, handle, 80, 0);
  await page.waitFor(`${sideWidth} === 300`, 5000);
  // And narrows it, down to the smallest width however far the pointer goes.
  await dragBy(page, handle, -1000, 0);
  await page.waitFor(`${sideWidth} === 140`, 5000);
  // Up to the largest.
  await dragBy(page, handle, 2000, 0);
  await page.waitFor(`${sideWidth} === 600`, 5000);
  await dragBy(page, handle, -300, 0);
  await page.waitFor(`${sideWidth} === 300`, 5000);

  // A builder opened later starts with the width the last one left.
  await menu(page, "orders", "Visual query builder", null, NAME);
  await page.waitFor(`${builderSql}.includes('FROM orders')`, 15_000);
  assert.equal(await page.eval(sideWidth), 300, "the width is kept for a new builder");

  // A double click on the handle goes back to the default.
  await page.eval(`${handle}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${sideWidth} === 220`, 5000);

  // The box of a table: resized by its corner grip, the SQL untouched.
  await page.eval(`[...${builder}.querySelectorAll('.builder-source')].find(r => r.textContent === 'customers').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${box} !== null`, 10_000);
  // The columns arrive after the box: the grip moves down until they are listed.
  await page.waitFor(`${box}.querySelectorAll('.builder-columns li').length === 6`, 10_000);
  const sqlBefore = await page.eval(builderSql);
  const before = await page.eval(boxSize);
  assert.equal(before.w, 200, "the default box width");
  const grip = `${box}.querySelector('.builder-box-grip')`;
  const g = await page.rectOf(grip);
  await page.drag(g.x + g.w / 2, g.y + g.h / 2, g.x + g.w / 2 + 70, g.y + g.h / 2 + 60);
  await page.waitFor(`${boxSize}.w === 270`, 5000);
  const grown = await page.eval(boxSize);
  assert.ok(grown.h >= before.h + 59, `taller: ${JSON.stringify([before, grown])}`);
  assert.equal(await page.eval(builderSql), sqlBefore, "resizing changes no SQL");
  // Never below the minimum.
  const g2 = await page.rectOf(grip);
  await page.drag(g2.x + g2.w / 2, g2.y + g2.h / 2, g2.x - 600, g2.y - 600);
  await page.waitFor(`${boxSize}.w === 140`, 5000);
  assert.ok((await page.eval(boxSize)).h >= 80, "not below the minimum height");
  // The box still drags by its header after a resize.
  const h = await page.rectOf(`${box}.querySelector('.builder-box-header')`);
  const at = await page.eval(`${box}.style.left`);
  await page.drag(h.x + 20, h.y + 10, h.x + 90, h.y + 40);
  await page.waitFor(`${box}.style.left !== ${JSON.stringify(at)}`, 5000);
  assert.equal(await page.eval(`document.querySelectorAll('.notice.error').length`), 0);
}
