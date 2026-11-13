// Regression guard (works): column statistics and bar charts agree with the database. Statistics
// of a text, an integer and a date-as-text column over all rows; over a result not loaded in full
// they say "of N loaded rows". A bar chart of 120 categories keeps the 50 largest and gathers the
// rest in "Other", with sums equal to SQL's.
import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { markTitles, liteConnection, newEditor, profileOf, runQuery, showChart, status, visibleTab } from "../analysis-helpers.mjs";

const NAME = "QA ana stats chart";
let file;

export function setup({ dir }) {
  file = liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000a0122",
    `CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT, d TEXT);
     INSERT INTO items VALUES (1, 1, 'a', '2024-01-01'), (2, 20, 'b', '2024-02-01'), (3, 1, 'a', '2023-12-31'),
       (4, -5, NULL, NULL), (5, 1, 'c', '2024-01-15');
     CREATE TABLE many (id INTEGER PRIMARY KEY, cat TEXT, v INTEGER);
     WITH RECURSIVE g(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM g WHERE n < 30000)
       INSERT INTO many SELECT n, 'c' || (n % 120), n % 120 FROM g;`,
  );
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT name, grp, d FROM items ORDER BY id");
  assert.equal(await page.eval(status), "5 rows");
  const name = await profileOf(page, "name");
  assert.deepEqual(name.fields, { "NULL values": "20%", Distinct: "3", Minimum: "a", Maximum: "c" });
  assert.deepEqual(name.top, ["a", "2", "b", "1", "c", "1"]);
  assert.equal(name.scope, "of all 5 rows");
  const grp = await profileOf(page, "grp");
  assert.deepEqual(grp.fields, { "NULL values": "0%", Distinct: "3", Minimum: "-5", Maximum: "20" });
  assert.equal(grp.bars, 20);
  const d = await profileOf(page, "d");
  assert.equal(d.fields.Minimum, "2023-12-31");
  assert.equal(d.fields.Maximum, "2024-02-01");

  // A result not loaded in full.
  await runQuery(page, "SELECT cat, v FROM many ORDER BY id");
  // The grid's first window asks for rows beyond it once the first page arrived: the count
  // is final when it stops growing.
  // English groups thousands: "1,500 rows loaded".
  const count = async () => Number((await page.eval(status)).match(/^([\d,]+) rows loaded/)?.[1].replaceAll(",", "") ?? 0);
  let loaded = await count();
  for (let i = 0; i < 20; i++) {
    await page.sleep(500);
    const now = await count();
    if (now > 0 && now === loaded) break;
    loaded = now;
  }
  assert.ok(loaded > 0 && loaded < 30000, await page.eval(status));
  const v = await profileOf(page, "v");
  // Numbers in the interface's language: English groups thousands.
  assert.equal(v.scope, `of ${loaded.toLocaleString("en-US")} loaded rows`);

  // 120 categories: 50 bars and Other, sums as SQL's over the loaded rows.
  await showChart(page, "rect.chart-bar");
  const bars = await markTitles(page, "rect.chart-bar");
  assert.equal(bars.length, 51);
  const db = new DatabaseSync(file, { readOnly: true });
  const sums = new Map(db.prepare(`SELECT cat, sum(v) AS s FROM (SELECT * FROM many ORDER BY id LIMIT ${loaded}) GROUP BY cat`).all().map((r) => [r.cat, r.s]));
  db.close();
  const total = [...sums.values()].reduce((a, b) => a + b, 0);
  let shown = 0;
  for (const title of bars) {
    const [, cat] = title.match(/^(.*) · v: (.*)$/);
    if (cat === "Other") continue;
    assert.ok(sums.has(cat), cat);
    shown += sums.get(cat);
  }
  assert.ok(bars.at(-1).startsWith("Other · v:"), bars.at(-1));
  assert.ok(total - shown > 0);
  assert.match(await page.eval(`${visibleTab}.querySelector('.chart-note').textContent`), /grouped to fit/);
}
