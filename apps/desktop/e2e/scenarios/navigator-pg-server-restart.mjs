// qa4-nav-3 (high). A PostgreSQL server that is stopped and started again is never reached
// again by the open session: the navigator's schema that failed to load while the server was
// down keeps its "error connecting to server … (os error 10061)" row, Refresh on a schema
// fails the same way, and so does "Open data", although the server answers again (a fresh
// connection works: Refresh on the connection, or Disconnect and open it, loads at once).
// Steps: connection to TimescaleDB (docker, port 55433, it is a PostgreSQL), expand schema
//   qa4_rs1; `docker stop` the server; expand qa4_rs2 (fails, as it must); `docker start`,
//   wait until psql answers and 3 s more; right-click qa4_rs2 › Refresh.
// Expected: qa4_rs2 loads its table and Open data shows its rows (the session reconnects, as
//   `live()` promises).
// Actual: schema Refresh may load, but "Open data" on qa4_rs1's table (or any statement run in a
//   tab of the connection, even `SELECT 1`) never returns, or fails with "error connecting to
//   server … (os error 10061)": the session is dead until Disconnect/Refresh of the connection.
// Code: apps/desktop/src-tauri/src/commands/mod.rs `live()` / the PostgreSQL driver's pool:
//   the session keeps answering with the failed connect, only a new session recovers.
// Overlaps the connections area (connection-reconnect-*); kept here because it is the navigator
// that stays broken. Needs the `pgwire` profile (timescale container).
import assert from "node:assert/strict";
import { spawnSync } from "../proc.mjs";
import { connect as tcp } from "node:net";
import { DBS, connectTo, expand, menu, row, saveConnections, visibleTab } from "../navigator-helpers.mjs";

export const skip = () =>
  new Promise((resolve) => {
    const s = tcp({ host: "127.0.0.1", port: 55433, timeout: 1000 });
    s.on("connect", () => { s.destroy(); resolve(null); });
    s.on("error", () => resolve("no TimescaleDB on port 55433 (docker compose --profile pgwire up -d)"));
    s.on("timeout", () => { s.destroy(); resolve("no TimescaleDB on port 55433"); });
  });

const psql = (sql) => spawnSync("docker", ["exec", "-i", "platypus-db-timescale-1", "psql", "-U", "platypus", "-d", "platypus", "-c", sql], { encoding: "utf8" });

export function setup({ dir }) {
  psql("CREATE SCHEMA IF NOT EXISTS qa4_rs1; CREATE SCHEMA IF NOT EXISTS qa4_rs2; CREATE TABLE IF NOT EXISTS qa4_rs1.t1 (id int); CREATE TABLE IF NOT EXISTS qa4_rs2.t2 (id int);");
  saveConnections(dir, DBS.postgres.connection("QA TS", "00000000-0000-4000-8000-0000000a4003").replace("55432", "55433"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA TS", "platypus");
    await expand(page, "qa4_rs1", "t1");
    spawnSync("docker", ["stop", "platypus-db-timescale-1"]);
    await page.eval(`${row("qa4_rs2")}.querySelector('.twisty').click()`);
    await page.waitFor(`${row("qa4_rs2")}?.classList.contains('failed')`, 20_000);
    // A statement tried while the server is down (it must fail).
    await menu(page, "t1", "Open data");
    await page.waitFor(`/error connecting|10061/.test(${visibleTab}?.querySelector('.statusbar')?.textContent ?? '') || document.querySelector('.notice.error') !== null`, 20_000).catch(() => {});
    await menu(page, "t1", "Open details");
    await menu(page, "qa4_rs2", "Refresh");
    await page.sleep(15_000);
    spawnSync("docker", ["start", "platypus-db-timescale-1"]);
    for (let i = 0; i < 40 && psql("select 1").status !== 0; i++) await page.sleep(1500);
    await page.sleep(3000);
    await menu(page, "qa4_rs2", "Refresh");
    const loaded = await page.waitFor(`${row("t2")} !== undefined`, 15_000).then(() => true, () => false);
    await menu(page, "t1", "Open data");
    const ran = await page.waitFor(`/rows?/.test(${visibleTab}?.querySelector('.statusbar')?.textContent ?? '')`, 15_000).then(() => true, () => false);
    assert.ok(loaded, "Refresh on the failed schema loads it once the server is back");
    assert.ok(ran, "Open data on t1 runs again once the server is back (the tab of the failed attempt is re-run)");
  } finally {
    spawnSync("docker", ["start", "platypus-db-timescale-1"]);
  }
}
