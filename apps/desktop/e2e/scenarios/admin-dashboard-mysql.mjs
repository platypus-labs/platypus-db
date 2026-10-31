// Group S against the dev MySQL 8 and MariaDB: Server activity's Dashboard (the global status
// counters, @@max_connections, information_schema sizes, SHOW REPLICA STATUS). The first
// reading has the gauges and the hit ratio but no rates, and says why; the second gives the
// rates; a session running a statement raises the running connections (with the server's
// maximum in the note), a stream of explicit transactions the transactions per second (MySQL
// counts COMMIT and ROLLBACK, not autocommit) and the statements per second (which MySQL does
// count, unlike PostgreSQL: no "—" there); the sparklines grow with every reading and are
// kept when the tab shows another view; the manual refresh adds a reading; the databases are
// listed with their sizes; the replication says it could not be read when the app user lacks the
// right (MySQL) and that the server follows no other once it has it (MariaDB). See the Dashboard
// component in src/admin/Dashboard.tsx.
import { spawnSync } from "../proc.mjs";
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

export const skip = needs("mysql", "mariadb");

const NAMES = { mysql: "QA S MySQL dashboard", mariadb: "QA S MariaDB dashboard" };

export function setup({ dir }) {
  connections(dir, [[NAMES.mysql, "mysql"], [NAMES.mariadb, "mariadb"]]);
}

const REPLICATION = { mysql: "REPLICATION CLIENT", mariadb: "SLAVE MONITOR" };

/** Runs `sql` as root in the container of `db`. */
function root(db, sql) {
  const r = spawnSync(
    "docker",
    ["exec", "-i", `platypus-db-${db}-1`, db === "mariadb" ? "mariadb" : "mysql", "-uroot", "-pplatypus"],
    { input: sql, encoding: "utf8" },
  );
  if (r.status !== 0) throw new Error(`${db} root: ${r.stderr || r.stdout}`);
}

/** The number a tile shows, or NaN for "—". */
const figure = async (page, id) => Number((await page.eval(tileValue(id))).replace(/[^\d.]/g, "") || NaN);

/** Brings the Server activity tab of `name` to the front. */
async function backToAdmin(page, name) {
  const title = JSON.stringify(`Server activity — ${name}`);
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.includes(${title})).click()`);
  await page.waitFor(`${adminTab} != null`, 15_000);
}

async function flow(page, db) {
  const name = NAMES[db];
  if (db === "mariadb") root(db, `GRANT ${REPLICATION[db]} ON *.* TO 'platypus'@'%';`);
  let load = null;
  let busy = null;
  try {
    await openAdmin(page, name, db);
    await showSection(page, "Dashboard");
    await page.waitFor(`${tileValue("uptime")} !== null && ${tileValue("uptime")} !== '—'`, 30_000);
    // A rate is the change between two readings.
    assert.equal(await page.eval(tileValue("transactions")), "—");
    assert.match(await page.eval(tileTitle("transactions")), /two readings/);
    assert.equal(await page.eval(tileValue("statements")), "—");
    assert.match(await page.eval(tileTitle("statements")), /two readings/);
    assert.match(await page.eval(tileValue("cache")), /^\d+\.\d%$/, "the InnoDB hit ratio since the start");
    assert.match(await page.eval(tileValue("uptime")), /^\d+(d \d+h|h \d+m|m \d+s|s)$/);
    assert.match(await page.eval(`${adminTab}.querySelector('.dash-tile[data-tile="connections"] .dash-note').textContent`), /max 151/);

    // The second reading: both rates, and the statements are counted here.
    await page.waitFor(`${tileValue("transactions")} !== '—' && ${tileValue("statements")} !== '—'`, 40_000);
    assert.ok((await figure(page, "transactions")) >= 0);
    assert.ok((await figure(page, "statements")) >= 0);

    // A session running a statement is a running connection (the app's own is left out).
    busy = background(db, "SELECT SLEEP(40);\n");
    await setRefresh(page, 2);
    await page.waitFor(`Number(${tileValue("connections")}) >= 1`, 30_000);

    // A stream of short transactions, each ended by a COMMIT: well over one a second, and each
    // statement counted.
    load = background(db, "START TRANSACTION; SELECT SLEEP(0.05); COMMIT;\n".repeat(400));
    await page.waitFor(`Number(${tileValue("transactions")}) >= 2`, 60_000);
    await page.waitFor(`Number(${tileValue("statements")}) >= 4`, 60_000);

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
    await page.sleep(1500);
    await page.waitFor(`${adminTab}.querySelector('button[aria-label="Refresh now"]').disabled === false`, 20_000);
    const manual = await page.eval(sparkPoints("transactions"));
    await page.eval(`${adminTab}.querySelector('button[aria-label="Refresh now"]').click()`);
    await page.waitFor(`${sparkPoints("transactions")} > ${manual}`, 20_000);

    // The databases with their bars, and no replication on this server.
    const names = await page.eval(
      `[...${adminTab}.querySelectorAll('[data-section="databases"] tbody tr')].map(r => r.dataset.name)`,
    );
    assert.ok(names.includes("platypus"), `databases: ${names}`);
    assert.ok(names.includes(db === "mysql" ? "sys" : "information_schema") || names.length >= 2, `databases: ${names}`);
    assert.ok(await page.eval(`${adminTab}.querySelector('[data-section="databases"] .dash-bar') !== null`));
    assert.equal(await page.eval(`${adminTab}.querySelector('[data-field="databases-unavailable"]') != null`), false);

    // Replication. MySQL's app user may not look at it (a session keeps the rights it had when it
    // connected): the section says why and the rest stays. MariaDB's was given the right before
    // connecting: a server that neither follows nor feeds another.
    const unavailable = `${adminTab}.querySelector('[data-field="replication-unavailable"]')`;
    const none = `${adminTab}.querySelector('[data-field="replication-none"]')`;
    if (db === "mysql") {
      assert.match(await page.eval(`${unavailable}?.textContent ?? ''`), /The replication could not be read: .*(Access denied|privilege)/);
      assert.equal(await page.eval(`${none} != null`), false);
    } else {
      assert.equal(await page.eval(`${unavailable} != null`), false);
      assert.equal(await page.eval(`${none} != null`), true);
    }
  } finally {
    if (db === "mariadb") root(db, `REVOKE ${REPLICATION[db]} ON *.* FROM 'platypus'@'%';`);
    busy?.child.kill();
    load?.child.kill();
    if (page) await backToAdmin(page, name).catch(() => {});
  }
}

export default async function (page) {
  await flow(page, "mysql");
  await flow(page, "mariadb");
}
