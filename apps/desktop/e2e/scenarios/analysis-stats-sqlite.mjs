// Column statistics: resting the mouse on a result column's header shows its NULLs, distinct
// values, extremes, most frequent values and over how many rows; moving away hides them.
import assert from "node:assert/strict";
import { openTable, runQuery, sqliteConnection, status, visibleTab } from "../query.mjs";

const NAME = "E2E column stats";
const profile = `${visibleTab}?.querySelector('.column-profile')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "stats.db",
    id: "00000000-0000-4000-8000-000000000031",
    name: NAME,
    sql: `CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT);
           INSERT INTO items VALUES (1, 1, 'a'), (2, 2, 'b'), (3, 1, 'a'), (4, 2, NULL), (5, 1, 'c');`,
  });
}

export default async function (page) {
  await openTable(page, NAME, "items", "5 rows");
  await runQuery(page, "SELECT name, grp FROM items");
  assert.equal(await page.eval(status), "5 rows");

  const grid = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  // Nothing until the mouse rests on a header.
  assert.equal(await page.eval(`${profile} ?? null`), null);
  await page.mouse("mouseMoved", grid.x + 40, grid.y + 14);
  await page.waitFor(`${profile}?.textContent.includes('of all 5 rows') === true`, 5_000);
  const text = await page.eval(`${profile}.textContent`);
  assert.match(text, /^name/);
  assert.match(text, /NULL values\s*20%/);
  assert.match(text, /Distinct\s*3/);
  assert.match(text, /Minimum\s*a/);
  assert.match(text, /Maximum\s*c/);
  assert.deepEqual(
    await page.eval(`[...${profile}.querySelectorAll('.column-profile-top span')].map(s => s.textContent)`),
    ["a", "2", "b", "1", "c", "1"],
  );

  // Away from the header, the box goes.
  await page.mouse("mouseMoved", grid.x + 40, grid.y + 120);
  await page.waitFor(`${profile} == null`);
}
