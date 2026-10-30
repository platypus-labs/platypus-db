// QA journey (works, regression guard): an analyst on PostgreSQL. 100,000 rows loaded in
// full; SQL on the result (a filter, then a GROUP BY on the loaded rows); the statistics of a
// column; the chart of the grouped rows exported to PNG; back to the full result; the data
// changes, the statement runs again and "Compare with previous run" finds the rows changed
// and the one removed.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { connectWith, needsPostgres, pgConnection, psql, psqlTry, status, treeRow, visibleTab } from "../qa-jrn.mjs";
import { fetchAll, run } from "../perf.mjs";
import { chart, compareButton, diffTab, fieldError, inField, mark, profileOf, toggle } from "../qa-ana.mjs";
import { writeFileSync } from "node:fs";

export const skip = needsPostgres;
const NAME = "QA jrn analyst";
const SQL = "select * from qa_jrn_sales order by id";

export function setup({ dir }) {
  psql(
    "SET lock_timeout = '10s'; DROP TABLE IF EXISTS qa_jrn_sales; CREATE TABLE qa_jrn_sales AS SELECT g AS id, 'r' || (g % 7) AS region, (g % 100) AS grp, " +
      "round(((g * 37) % 1000)::numeric / 3, 2) AS amount FROM generate_series(1, 100000) g; ALTER TABLE qa_jrn_sales ADD PRIMARY KEY (id);",
  );
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${pgConnection("00000000-0000-4000-8000-0000000a9001", NAME)}`);
}

export default async function (page, ctx) {
  try {
    await connectWith(page, NAME);
    await page.eval(`${treeRow(NAME)}.id = 'jrn-c'`);
    await page.clickSel("#jrn-c", { clickCount: 2 });
    await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
    await run(page, SQL);
    await fetchAll(page);
    assert.match(await page.eval(status), /^100[.,]?000 rows$/);

    // SQL on the loaded rows.
    const expected = psql("SELECT count(*) FROM qa_jrn_sales WHERE grp = 5 AND region = 'r3'");
    const filtered = await inField(page, "WHERE grp = 5 AND region = 'r3'");
    assert.equal(filtered.error, "", await page.eval(fieldError));
    assert.equal(filtered.mark, `SQL view · ${expected} rows`);
    const grouped = await inField(page, "SELECT region, count(*) AS n, sum(amount) AS total FROM result GROUP BY region ORDER BY region");
    assert.equal(grouped.mark, "SQL view · 7 rows");
    const profile = await profileOf(page, "n");
    assert.equal(profile.fields.Distinct, "2");
    assert.equal(profile.scope, "of all 7 rows");

    // The chart, exported.
    await page.eval(`${toggle("Chart")}.click()`);
    await page.waitFor(`${chart}?.querySelectorAll('rect.chart-bar').length === 14`, 10_000);
    const png = join(ctx.dir, "chart.png");
    await page.eval(`window.__platypus.answer("file", ${JSON.stringify(png)})`);
    await page.eval(`[...${visibleTab}.querySelectorAll('.chart-export button')].find(b => b.textContent === 'PNG image…').click()`);
    const deadline = Date.now() + 10_000;
    let bytes = null;
    while (Date.now() < deadline && !bytes) {
      try {
        bytes = readFileSync(png);
      } catch {
        await page.sleep(200);
      }
    }
    assert.deepEqual([...bytes.subarray(0, 4)], [0x89, 0x50, 0x4e, 0x47]);
    await page.eval(`${toggle("Grid")}.click()`);

    // Back to the full result.
    await page.eval(`[...${visibleTab}.querySelectorAll('.result-sql button')].find(b => b.classList.contains('result-sql-clear')).click()`);
    await page.waitFor(`/^100[.,]?000 rows$/.test(${status})`, 10_000);

    // The data changes; the same statement again; the comparison.
    psql("UPDATE qa_jrn_sales SET amount = amount + 1 WHERE id <= 10; DELETE FROM qa_jrn_sales WHERE id = 99999");
    await run(page, SQL);
    await fetchAll(page);
    assert.match(await page.eval(status), /^99[.,]?999 rows$/);
    await page.waitFor(`${compareButton}?.disabled === false`, 10_000);
    await page.eval(`${compareButton}.click()`);
    await page.waitFor(`${diffTab}?.querySelector('.diff-counts') != null`, 60_000);
    assert.equal(await page.eval(`${diffTab}.querySelector('.diff-counts').textContent`), "+0 −1 ~10");
  } finally {
    psqlTry("SET lock_timeout = '10s'; DROP TABLE IF EXISTS qa_jrn_sales");
  }
}
