// Group J against the dev PostgreSQL: Server activity's Dashboard (pg_stat_activity,
// pg_stat_database, pg_stat_replication). The first reading has no rates and says why; the
// second gives them; a session of the shell raises the running connections, and a stream of
// short statements the transactions per second; the sparklines grow with every reading and
// are kept when the tab shows another view; PostgreSQL's missing statement counter shows "—"
// with its reason; the databases are listed with their sizes and a development server has no
// replication. See the Dashboard component in src/admin/Dashboard.tsx.
import assert from "node:assert/strict";
import { connections, needs } from "../qa-grid.mjs";
import {
  adminTab,
  background,
  openAdmin,
  setRefresh,
  showSection,
  sparkPoints,
  tile,
  tileTitle,
  tileValue,
} from "../admin.mjs";

const DB = "pg";
const NAME = "QA J PostgreSQL dashboard";

export const skip = needs(DB);

export function setup({ dir }) {
  connections(dir, [[NAME, DB]]);
}

/** The number a tile shows, or NaN for "—". */
const figure = async (page, id) => Number((await page.eval(tileValue(id))).replace(/[^\d.]/g, "") || NaN);

export default async function (page) {
  let load = null;
  let busy = null;
  try {
    await openAdmin(page, NAME, DB);
    // The tab refreshes every 5 s: the first reading has gauges but no rates yet.
    await showSection(page, "Dashboard");
    await page.waitFor(`${tileValue("uptime")} !== null && ${tileValue("uptime")} !== '—'`, 30_000);
    assert.equal(await page.eval(tileValue("transactions")), "—", "a rate needs two readings");
    assert.match(await page.eval(tileTitle("transactions")), /two readings/);
    // The reason for "—" is on the tile, not only in a tooltip.
    assert.match(
      await page.eval(`${adminTab}.querySelector('.dash-tile[data-tile="transactions"] [data-field="reason"]').textContent`),
      /two readings/,
    );
    // The hit ratio since the start is known from the first reading.
    assert.match(await page.eval(tileValue("cache")), /^\d+\.\d%$/);
    assert.match(await page.eval(tileValue("uptime")), /^\d+(d \d+h|h \d+m|m \d+s|s)$/);

    // The second reading: the rate of transactions.
    await page.waitFor(`${tileValue("transactions")} !== '—'`, 40_000);
    assert.ok((await figure(page, "transactions")) >= 0);

    // A session running a statement is a running connection (the app's own is left out).
    busy = background(DB, "SELECT pg_sleep(40);\n");
    await setRefresh(page, 2);
    await page.waitFor(`${tileValue("connections")} !== null && Number(${tileValue("connections")}) >= 1`, 30_000);
    assert.match(await page.eval(`${adminTab}.querySelector('.dash-tile[data-tile="connections"] .dash-note').textContent`), /max 100/);
    // The tile says its count leaves the app's own session out, and each sparkline its span.
    assert.match(
      await page.eval(`${adminTab}.querySelector('.dash-tile[data-tile="connections"] .dash-note').textContent`),
      /not counting this app's own session/,
    );
    assert.match(
      await page.eval(`${adminTab}.querySelector('.dash-tile[data-tile="connections"] [data-field="spark-caption"]').textContent`),
      /^last \d+ (s|min) · max \d+$/,
    );

    // A stream of short statements, each its own transaction: well over one a second.
    load = background(DB, "SELECT pg_sleep(0.05);\n".repeat(400));
    await page.waitFor(`Number(${tileValue("transactions")}) >= 2`, 60_000);
    // The statements PostgreSQL does not count show "—", with the reason in the tooltip.
    assert.equal(await page.eval(tileValue("statements")), "—");
    assert.match(await page.eval(tileTitle("statements")), /PostgreSQL does not count it/);

    // The sparkline draws a point per reading, and keeps drawing.
    await page.waitFor(`${sparkPoints("transactions")} >= 2`, 30_000);
    const points = await page.eval(sparkPoints("transactions"));
    await page.waitFor(`${sparkPoints("transactions")} > ${points}`, 30_000);

    // Another view and back: the readings are kept.
    const kept = await page.eval(sparkPoints("transactions"));
    await showSection(page, "Sessions");
    await page.waitFor(`${adminTab}.querySelector('input[type=search]') !== null`);
    await showSection(page, "Dashboard");
    await page.waitFor(`${tile("transactions")} != null && ${sparkPoints("transactions")} >= ${kept}`, 15_000);

    // By hand: the interval off, and "Refresh now" adds a reading.
    await setRefresh(page, 0);
    // Changing the interval reads once more; let that reading end before counting.
    await page.sleep(1500);
    await page.waitFor(`${adminTab}.querySelector('button[aria-label="Refresh now"]').disabled === false`, 20_000);
    const manual = await page.eval(sparkPoints("transactions"));
    await page.eval(`${adminTab}.querySelector('button[aria-label="Refresh now"]').click()`);
    await page.waitFor(`${sparkPoints("transactions")} > ${manual}`, 20_000);

    // The databases, the biggest first with its bar, and no replication on this server.
    const names = await page.eval(
      `[...${adminTab}.querySelectorAll('[data-section="databases"] tbody tr')].map(r => r.dataset.name)`,
    );
    assert.equal(names[0], "platypus", `databases: ${names}`);
    assert.ok(await page.eval(`${adminTab}.querySelector('[data-section="databases"] .dash-bar') !== null`));
    assert.ok(await page.eval(`${adminTab}.querySelector('[data-field="replication-none"]') !== null`));
    assert.equal(await page.eval(`${adminTab}.querySelector('[data-field="databases-unavailable"]')`), null);
  } finally {
    busy?.child.kill();
    load?.child.kill();
  }
}
