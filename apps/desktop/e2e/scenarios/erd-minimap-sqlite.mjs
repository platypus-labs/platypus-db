// The minimap of the ER diagram. A small schema fits the canvas: no minimap. A wide one (30
// tables) overflows it: the minimap shows one rectangle per table and a frame for the visible
// part, at the bottom right and away from the floating toolbar. Clicking in it scrolls the
// canvas there, dragging follows the pointer, and zooming in keeps the frame inside it.
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, buttonIn, openPath, rowMenu, saveEngines, visibleErd } from "../qa3-schema.mjs";

const TABLES = 30;

export function setup({ dir }) {
  const sql = [`CREATE TABLE root (id INTEGER PRIMARY KEY);`];
  for (let i = 0; i < TABLES - 1; i++) sql.push(`CREATE TABLE t${i} (id INTEGER PRIMARY KEY, root_id INTEGER REFERENCES root(id));`);
  ENGINES.sqlite.seed(dir, SRC, sql.join("\n"));
  ENGINES.sqlite.seed(dir, DST, "CREATE TABLE only_one (id INTEGER PRIMARY KEY);");
  saveEngines(dir, ["sqlite"]);
}

const mini = `${visibleErd}.querySelector('.erd-minimap')`;
const canvas = `${visibleErd}.querySelector('.erd-canvas')`;
const scroll = `({ x: ${canvas}.scrollLeft, y: ${canvas}.scrollTop })`;

export default async function (page) {
  // Both connections expanded, the first one first: a row is found as the first "main" after
  // its connection, which would be the other's if its own were not shown.
  await openPath(page, ["QA3 SQLite src", "main"]);
  await openPath(page, ["QA3 SQLite dst", "main"]);

  // A diagram that fits has no minimap.
  await rowMenu(page, ["QA3 SQLite dst"], "main", "ER diagram");
  await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === 1`, 20_000);
  await page.sleep(300);
  assert.equal(await page.eval(`${mini} === null`), true, "a diagram that fits shows no minimap");

  // A wide one does.
  await rowMenu(page, ["QA3 SQLite src"], "main", "ER diagram");
  await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === ${TABLES}`, 20_000);
  await page.waitFor(`${mini} !== null`, 5000);
  assert.equal(await page.eval(`${mini}.querySelectorAll('i').length`), TABLES, "one rectangle per table");

  const geometry = `(() => { const m = ${mini}.getBoundingClientRect(), c = ${canvas}.getBoundingClientRect(), v = ${mini}.querySelector('.erd-minimap-view').getBoundingClientRect(), f = ${visibleErd}.querySelector('.erd-float').getBoundingClientRect(); return { m: { x: m.x, y: m.y, w: m.width, h: m.height, r: m.right, b: m.bottom }, c: { r: c.right, b: c.bottom }, v: { x: v.x, y: v.y, r: v.right, b: v.bottom }, f: { r: f.right, b: f.bottom } }; })()`;
  const g = await page.eval(geometry);
  assert.ok(g.m.r <= g.c.r && g.m.b <= g.c.b, "the minimap sits inside the canvas, at the bottom right");
  assert.ok(g.m.x > g.f.r || g.m.y > g.f.b, "the minimap does not cover the floating toolbar");
  assert.ok(g.m.w < 200 && g.m.h < 200, "the minimap is small");
  assert.ok(g.v.x >= g.m.x - 1 && g.v.y >= g.m.y - 1 && g.v.r <= g.m.r + 1 && g.v.b <= g.m.b + 1, "the frame is inside the minimap");

  // Scrolling the canvas moves the frame.
  await page.eval(`${canvas}.scrollLeft = 0; ${canvas}.scrollTop = 0`);
  await page.sleep(200);
  const home = await page.eval(geometry);
  await page.eval(`${canvas}.scrollTop = ${canvas}.scrollHeight`);
  await page.sleep(200);
  const down = await page.eval(geometry);
  assert.ok(down.v.y > home.v.y || down.v.x > home.v.x, "the frame follows the scroll");

  // A click at the end of the minimap scrolls the canvas to the end; one at its start, back.
  await page.mouse("mousePressed", g.m.x + g.m.w - 2, g.m.y + g.m.h - 2);
  await page.mouse("mouseReleased", g.m.x + g.m.w - 2, g.m.y + g.m.h - 2);
  await page.sleep(200);
  const end = await page.eval(`(() => { const c = ${canvas}; return { x: c.scrollLeft, y: c.scrollTop, mx: c.scrollWidth - c.clientWidth, my: c.scrollHeight - c.clientHeight }; })()`);
  assert.ok(end.x >= end.mx - 2 && end.y >= end.my - 2, `a click at the end scrolls to the end: ${JSON.stringify(end)}`);
  await page.mouse("mousePressed", g.m.x + 2, g.m.y + 2);
  await page.mouse("mouseReleased", g.m.x + 2, g.m.y + 2);
  await page.sleep(200);
  assert.deepEqual(await page.eval(scroll), { x: 0, y: 0 }, "a click at the start scrolls back");

  // Dragging in it follows the pointer: from the start to the middle of the minimap.
  await page.mouse("mousePressed", g.m.x + 2, g.m.y + 2);
  await page.mouse("mouseMoved", g.m.x + g.m.w / 4, g.m.y + g.m.h / 4, { buttons: 1 });
  await page.mouse("mouseMoved", g.m.x + g.m.w / 2, g.m.y + g.m.h / 2, { buttons: 1 });
  await page.mouse("mouseReleased", g.m.x + g.m.w / 2, g.m.y + g.m.h / 2);
  await page.sleep(200);
  const mid = await page.eval(scroll);
  assert.ok(mid.x > 0 || mid.y > 0, `a drag scrolls the canvas: ${JSON.stringify(mid)}`);
  assert.ok(mid.x < end.mx || mid.y < end.my, "a drag to the middle does not reach the end");
  assert.equal(await page.eval(`${visibleErd}.querySelectorAll('.erd-table').length`), TABLES, "no table moved or vanished");

  // Zooming out until it all fits hides the minimap; zooming back shows it again.
  for (let i = 0; i < 6; i++) {
    await page.eval(`${buttonIn(visibleErd, "Zoom out")}.click()`);
    await page.sleep(150);
  }
  await page.waitFor(`${mini} === null`, 5000);
  for (let i = 0; i < 6; i++) {
    await page.eval(`${buttonIn(visibleErd, "Zoom in")}.click()`);
    await page.sleep(150);
  }
  await page.waitFor(`${mini} !== null`, 5000);
}
