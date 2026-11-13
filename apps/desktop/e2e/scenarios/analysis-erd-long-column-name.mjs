// BUG qa4-ana-5 (low, visual): in the ER diagram a long column name runs over its type and past the
// box's right edge ("…forever" is drawn on top of "TEXT", outside the table).
// Steps: SQLite table with a column named "a very long column name that goes on and on and on and
// on forever" (TEXT); open the schema's ER diagram.
// Expected: the name is cut with an ellipsis (as the table's title is) or the box widens: no text
// overlaps another or leaves its table's rectangle.
// Actual: the name overlaps the type text and ends outside the box.
// Code: apps/desktop/src/ (ER diagram table drawing): the box width follows the title only.
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, openPath, rowMenu, saveEngines, visibleErd } from "../schema-compare-helpers.mjs";

export function setup({ dir }) {
  ENGINES.sqlite.seed(
    dir,
    SRC,
    `CREATE TABLE "a very long table name" (id INTEGER PRIMARY KEY, "a very long column name that goes on and on and on and on forever" TEXT);`,
  );
  ENGINES.sqlite.seed(dir, DST, "");
  saveEngines(dir, ["sqlite"]);
}

export default async function (page) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", "ER diagram");
  await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === 1`, 30_000);
  await page.sleep(500);
  const bad = await page.eval(`(() => {
    const g = ${visibleErd}.querySelector('.erd-table');
    const box = g.querySelector('rect').getBBox();
    return [...g.querySelectorAll('text')].map(t => t.getBBox()).filter(b => b.x + b.width > box.x + box.width + 0.5).length;
  })()`);
  assert.equal(bad, 0, "no text leaves the table's rectangle");
  const overlap = await page.eval(`(() => {
    // Screen boxes: each column row is a group of its own, so getBBox would compare local
    // coordinates of different rows.
    const t = [...${visibleErd}.querySelector('.erd-table').querySelectorAll('text')].map(e => e.getBoundingClientRect());
    let n = 0;
    for (let i = 0; i < t.length; i++) for (let j = i + 1; j < t.length; j++) {
      const a = t[i], b = t[j];
      if (a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom) n++;
    }
    return n;
  })()`);
  assert.equal(overlap, 0, "no two texts overlap");
}
