// Group I: the ER diagram in full screen, and a Refresh that keeps the positions. A diagram of a
// SQLite schema: one table dragged, a table added outside the app, then Refresh: every table
// keeps its exact transform (the dragged one and the laid-out ones), the new tables appear
// below them without overlapping any box. Auto layout still places everything again. Full
// screen covers the window, from the button and left with it or with Escape; Escape in the
// note dialog closes only the dialog.
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, buttonIn, openPath, rowMenu, saveEngines, visibleErd } from "../qa3-schema.mjs";

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, `CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));
CREATE TABLE order_items (id INTEGER PRIMARY KEY, order_id INTEGER REFERENCES orders(id));
CREATE TABLE audit (id INTEGER PRIMARY KEY, what TEXT);
CREATE TABLE settings (k TEXT, v TEXT);`);
  ENGINES.sqlite.seed(dir, DST, "");
  saveEngines(dir, ["sqlite"]);
}

/** Every drawn table: its name, transform and box, as the page shows them. */
const boxes = `[...${visibleErd}.querySelectorAll('.erd-table')].map(g => {
  const r = g.querySelector('rect');
  const m = /translate\\(([-\\d.]+),\\s*([-\\d.]+)\\)/.exec(g.getAttribute('transform'));
  return { name: g.querySelector('text').textContent, transform: g.getAttribute('transform'), x: +m[1], y: +m[2], w: +r.getAttribute('width'), h: +r.getAttribute('height') };
})`;
const isFull = `${visibleErd}.classList.contains('erd-full')`;
const modal = `document.querySelector('.modal')`;

/** The boxes as a name -> transform map. */
const transforms = async (page) => Object.fromEntries((await page.eval(boxes)).map((b) => [b.name, b.transform]));

/** Asserts that no two boxes of the diagram overlap. */
function assertDisjoint(list, what) {
  for (const [i, a] of list.entries()) {
    for (const b of list.slice(i + 1)) {
      const apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
      assert.ok(apart, `${what}: ${a.name} overlaps ${b.name}: ${JSON.stringify([a, b])}`);
    }
  }
}

/** A move of table `name`, in screen pixels, that lands it on empty space inside the diagram,
 *  as close to (`dx`, `dy`) as there is: where the layout puts the other tables depends on
 *  the window's size, and a box dropped on another would fail the overlap checks. */
async function freeMove(page, name, dx, dy) {
  return page.eval(`(() => {
    const erd = ${visibleErd};
    // The part of the drawing the canvas shows, without the toolbar and minimap over it.
    const [c, s] = [erd.querySelector('.erd-canvas').getBoundingClientRect(), erd.querySelector('.erd-canvas > svg').getBoundingClientRect()];
    const area = { x: Math.max(c.x, s.x), y: Math.max(c.y, s.y), right: Math.min(c.right, s.right), bottom: Math.min(c.bottom, s.bottom) };
    area.width = area.right - area.x;
    area.height = area.bottom - area.y;
    const rects = [...erd.querySelectorAll('.erd-table')].map(g => ({ name: g.querySelector('text').textContent, r: g.querySelector('rect').getBoundingClientRect() }))
      .concat([...erd.querySelectorAll('.erd-float, .erd-minimap')].map(e => ({ name: '', r: e.getBoundingClientRect() })));
    const own = rects.find(t => t.name === ${JSON.stringify(name)}).r;
    const gap = 12;
    const free = (mx, my) => {
      const [x, y] = [own.x + mx, own.y + my];
      if (x < area.x + gap || y < area.y + gap || x + own.width > area.right - gap || y + own.height > area.bottom - gap) return false;
      return rects.every(t => t.name === ${JSON.stringify(name)} || x + own.width + gap <= t.r.x || t.r.right + gap <= x || y + own.height + gap <= t.r.y || t.r.bottom + gap <= y);
    };
    const moves = [];
    for (let mx = -area.width; mx <= area.width; mx += 20) for (let my = -area.height; my <= area.height; my += 20) moves.push([mx, my]);
    moves.sort((a, b) => Math.hypot(a[0] - ${dx}, a[1] - ${dy}) - Math.hypot(b[0] - ${dx}, b[1] - ${dy}));
    return moves.find(([mx, my]) => (mx !== 0 || my !== 0) && free(mx, my)) ?? null;
  })()`);
}

async function drag(page, name, wantDx, wantDy) {
  const move = await freeMove(page, name, wantDx, wantDy);
  assert.ok(move, `no empty space to drag ${name} to`);
  const [dx, dy] = move;
  const before = (await transforms(page))[name];
  await page.eval(`[...${visibleErd}.querySelectorAll('.erd-table')].find(g => g.querySelector('text').textContent === ${JSON.stringify(name)}).querySelector('rect').id = 'erd-drag'`);
  const r = await page.rect("#erd-drag");
  await page.eval(`document.getElementById('erd-drag').removeAttribute('id')`);
  await page.mouse("mousePressed", r.x + 20, r.y + 10);
  await page.mouse("mouseMoved", r.x + 20 + dx / 2, r.y + 10 + dy / 2, { buttons: 1 });
  await page.mouse("mouseMoved", r.x + 20 + dx, r.y + 10 + dy, { buttons: 1 });
  await page.mouse("mouseReleased", r.x + 20 + dx, r.y + 10 + dy);
  await page.waitFor(`${boxes}.find(b => b.name === ${JSON.stringify(name)}).transform !== ${JSON.stringify(before)}`, 5000);
}

/** Clicks Refresh and waits until the diagram shows exactly `count` tables. Every call here
 *  changes the number of tables, except where `same` says it does not: then the read is only
 *  given time to finish, and the positions are the same before and after either way. */
async function refresh(page, count, same = false) {
  await page.eval(`${buttonIn(visibleErd, "Refresh")}.click()`);
  await page.waitFor(`${visibleErd}.querySelectorAll('.erd-table').length === ${count}`, 15_000);
  if (same) await page.sleep(1500);
}

export default async function (page, { dir }) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", "ER diagram");
  await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === 5`, 20_000);
  assertDisjoint(await page.eval(boxes), "the first layout");

  // Dragging one table: the others stay where the layout put them.
  const laidOut = await transforms(page);
  await drag(page, "audit", 260, 140);
  const dragged = await transforms(page);
  assert.notEqual(dragged.audit, laidOut.audit);
  for (const name of Object.keys(laidOut).filter((n) => n !== "audit")) assert.equal(dragged[name], laidOut[name], `${name} moved with audit`);
  assertDisjoint(await page.eval(boxes), "after the drag");

  // A table added outside the app (one with a foreign key, one alone), then Refresh.
  ENGINES.sqlite.exec(dir, SRC, `CREATE TABLE invoices (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));
CREATE TABLE notes_alone (id INTEGER PRIMARY KEY);`);
  await refresh(page, 7);
  const after = await page.eval(boxes);
  const byName = Object.fromEntries(after.map((b) => [b.name, b]));
  for (const [name, transform] of Object.entries(dragged)) assert.equal(byName[name].transform, transform, `${name} jumped on Refresh`);
  assertDisjoint(after, "after Refresh");
  const placedBottom = Math.max(...Object.keys(dragged).map((n) => byName[n].y + byName[n].h));
  for (const name of ["invoices", "notes_alone"]) assert.ok(byName[name].y >= placedBottom, `${name} is below the placed tables: ${JSON.stringify(byName[name])}`);
  assert.equal(await page.eval(`${visibleErd}.querySelectorAll('.notice.error').length`), 0);

  // A table dropped outside: the others do not move when it goes.
  ENGINES.sqlite.exec(dir, SRC, `DROP TABLE settings;`);
  await refresh(page, 6);
  const dropped = await transforms(page);
  for (const [name, transform] of Object.entries(after.reduce((o, b) => ({ ...o, [b.name]: b.transform }), {}))) {
    if (name !== "settings") assert.equal(dropped[name], transform, `${name} jumped when settings went`);
  }

  // Auto layout places everything again (the dragged table goes back); a Refresh keeps even
  // that automatic layout, with nothing dragged.
  await page.eval(`${buttonIn(visibleErd, "Auto layout")}.click()`);
  await page.waitFor(`${boxes}.find(b => b.name === 'audit').transform !== ${JSON.stringify(dropped.audit)}`, 5000);
  assertDisjoint(await page.eval(boxes), "after Auto layout");
  const auto = await transforms(page);
  await refresh(page, 6, true);
  assert.deepEqual(await transforms(page), auto, "Refresh keeps the automatic layout");

  // Full screen: from the button, covering the whole window.
  assert.equal(await page.eval(isFull), false);
  assert.ok(await page.eval(`!!${buttonIn(visibleErd, "Full screen")}`));
  await page.eval(`${buttonIn(visibleErd, "Full screen")}.click()`);
  await page.waitFor(isFull);
  const cover = await page.eval(`(() => { const r = ${visibleErd}.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height, iw: innerWidth, ih: innerHeight }; })()`);
  assert.deepEqual([cover.x, cover.y, cover.w, cover.h], [0, 0, cover.iw, cover.ih], "the diagram covers the window");
  assert.equal(await page.eval(`document.elementFromPoint(40, 100).closest('.erd') === ${visibleErd}`), true, "nothing of the app is over it");
  assert.equal(await page.eval(`!!${buttonIn(visibleErd, "Exit full screen")} && ${buttonIn(visibleErd, "Full screen")} === undefined`), true);
  assert.deepEqual(await transforms(page), auto, "full screen moves no table");
  await page.eval(`${buttonIn(visibleErd, "Exit full screen")}.click()`);
  await page.waitFor(`!(${isFull})`);
  assert.ok(await page.eval(`${visibleErd}.getBoundingClientRect().height < innerHeight`), "back in its place");

  // Escape leaves it.
  await page.eval(`${buttonIn(visibleErd, "Full screen")}.click()`);
  await page.waitFor(isFull);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`!(${isFull})`);

  // Dialogs of the diagram show above it, and Escape in one closes only the dialog.
  await page.eval(`${buttonIn(visibleErd, "Full screen")}.click()`);
  await page.waitFor(isFull);
  await page.eval(`${buttonIn(visibleErd, "Add note")}.click()`);
  await page.waitFor(`${modal}?.querySelector('textarea') != null`);
  const shown = await page.eval(`(() => { const r = ${modal}.getBoundingClientRect(); return document.elementFromPoint(r.x + r.width / 2, r.y + 20)?.closest('.modal') === ${modal}; })()`);
  assert.equal(shown, true, "the note dialog is above the full-screen diagram");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${modal} === null`);
  await page.sleep(300);
  assert.equal(await page.eval(isFull), true, "Escape in the dialog left the full screen alone");
  await page.eval(`${buttonIn(visibleErd, "Add tables")}.click()`);
  await page.waitFor(`document.querySelector('.modal.erd-tables') != null`);
  const picker = await page.eval(`(() => { const m = document.querySelector('.modal.erd-tables'); const r = m.getBoundingClientRect(); return document.elementFromPoint(r.x + r.width / 2, r.y + 20)?.closest('.modal') === m; })()`);
  assert.equal(picker, true, "the Tables dialog is above the full-screen diagram");
  await page.key("Escape", "Escape", 27);
  await page.sleep(300);
  assert.equal(await page.eval(isFull), true, "Escape in the Tables dialog left the full screen alone");
  // The dialog may handle Escape itself; if it is still there, Cancel closes it.
  await page.eval(`(() => { const m = document.querySelector('.modal.erd-tables'); if (m) ${buttonIn("m", "Cancel")}.click(); })()`);
  await page.waitFor(`document.querySelector('.modal.erd-tables') === null`);

  // Refresh in full screen keeps the positions too; then a last Escape leaves.
  await refresh(page, 6, true);
  assert.deepEqual(await transforms(page), auto);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`!(${isFull})`);
}
