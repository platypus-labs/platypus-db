// UX of the ER diagram on SQLite (40 tables, so that the diagram overflows its canvas): the
// button and the dialog that pick the tables say "Add tables"; a pick of 5 tables shows
// exactly those 5 and the loading state is gone once they are drawn; a pick of none cannot be
// shown; picking them all again shows the 40; dragging on the empty canvas scrolls the view
// (and does not move a table), dragging a table still moves the table only.
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, buttonIn, openPath, rowMenu, saveEngines, visibleErd } from "../schema-compare-helpers.mjs";

const TABLES = 40;
const PICKED = ["t3", "t7", "t11", "t19", "t33"];

export function setup({ dir }) {
  const sql = [`CREATE TABLE root (id INTEGER PRIMARY KEY);`];
  for (let i = 0; i < TABLES - 1; i++) sql.push(`CREATE TABLE t${i} (id INTEGER PRIMARY KEY, root_id INTEGER REFERENCES root(id));`);
  ENGINES.sqlite.seed(dir, SRC, sql.join("\n"));
  ENGINES.sqlite.seed(dir, DST, "");
  saveEngines(dir, ["sqlite"]);
}

const canvas = `${visibleErd}.querySelector('.erd-canvas')`;
const drawn = `${visibleErd}.querySelectorAll('.erd-table').length`;
const dialog = `document.querySelector('.modal.erd-tables')`;
const names = `[...${visibleErd}.querySelectorAll('.erd-table')].map(g => g.querySelector('text').textContent).sort()`;
const dialogButton = (text) => `[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const view = `({ x: ${canvas}.scrollLeft, y: ${canvas}.scrollTop })`;

/** Opens the picker with its button. */
async function openPicker(page) {
  await page.eval(`${buttonIn(visibleErd, "Add tables…")}.click()`);
  await page.waitFor(`${dialog} !== null`, 5000);
}

export default async function (page) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", "ER diagram");
  await page.waitFor(`${drawn} === ${TABLES}`, 30_000);

  // The picker is called "Add tables", the old label is gone.
  assert.equal(await page.eval(`${buttonIn(visibleErd, "Tables…")} == null`), true, "no button called just Tables");
  await openPicker(page);
  assert.equal(await page.eval(`${dialog}.querySelector('h2').textContent`), "Add tables to the diagram");

  // None chosen: Show is off. Then five, found with the filter.
  await page.eval(`${dialogButton("Clear shown")}.click()`);
  await page.waitFor(`${dialogButton("Show")}.disabled`, 5000);
  for (const name of PICKED) {
    await page.eval(`(() => { const l = [...${dialog}.querySelectorAll('.erd-table-list label')].find(l => l.textContent.trim() === ${JSON.stringify(name)}); l.querySelector('input').click(); })()`);
  }
  await page.waitFor(`!${dialogButton("Show")}.disabled && ${dialog}.textContent.includes('${PICKED.length} tables chosen')`, 5000);
  await page.eval(`${dialogButton("Show")}.click()`);
  // Exactly the five, none missing, and nothing is still loading.
  await page.waitFor(`${drawn} === ${PICKED.length}`, 30_000);
  assert.deepEqual(await page.eval(names), [...PICKED].sort());
  await page.waitFor(`${visibleErd}.querySelector('.erd-loading') === null`, 10_000);

  // All of them again.
  await openPicker(page);
  await page.eval(`${dialogButton("Select shown")}.click()`);
  await page.eval(`${dialogButton("Show")}.click()`);
  await page.waitFor(`${drawn} === ${TABLES}`, 30_000);
  await page.waitFor(`${visibleErd}.querySelector('.erd-loading') === null`, 10_000);

  // Dragging on the empty canvas moves the view. A point of the background, below the toolbar.
  await page.eval(`${canvas}.scrollTo(0, 0)`);
  await page.sleep(200);
  const spot = await page.eval(`(() => {
    const r = ${canvas}.getBoundingClientRect();
    for (let y = r.top + 100; y < r.bottom - 20; y += 12) {
      for (let x = r.left + 20; x < r.right - 20; x += 12) {
        const el = document.elementFromPoint(x, y);
        if (el?.classList.contains('erd-bg')) return { x, y };
      }
    }
    return null;
  })()`);
  assert.ok(spot, "there is an empty spot on the canvas");
  const before = await page.eval(view);
  const tables = await page.eval(`${visibleErd}.querySelectorAll('.erd-table')[0].getAttribute('transform')`);
  await page.mouse("mousePressed", spot.x, spot.y);
  await page.mouse("mouseMoved", spot.x - 30, spot.y - 20, { buttons: 1 });
  await page.mouse("mouseMoved", spot.x - 60, spot.y - 40, { buttons: 1 });
  await page.mouse("mouseReleased", spot.x - 60, spot.y - 40);
  await page.waitFor(`${view}.x > ${before.x} || ${view}.y > ${before.y}`, 5000);
  const after = await page.eval(view);
  assert.ok(after.x >= before.x && after.y >= before.y && after.x + after.y > before.x + before.y, `the view moved: ${JSON.stringify([before, after])}`);
  assert.equal(await page.eval(`${visibleErd}.querySelectorAll('.erd-table')[0].getAttribute('transform')`), tables, "panning moves no table");

  assert.equal(await page.eval(`${visibleErd}.querySelectorAll('.notice.error').length`), 0);
}
