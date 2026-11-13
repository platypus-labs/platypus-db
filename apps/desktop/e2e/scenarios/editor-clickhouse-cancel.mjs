// SQL-21. ClickHouse: a slow query cannot be cancelled once its first (even empty) block
// arrives. The status turns to "0 rows loaded · more available", Cancel is disabled, and the
// query goes on running on the server, its rows trickling into the grid ("4 rows loaded…").
// Steps: ClickHouse, Ctrl+Enter on `SELECT sleepEachRow(1) ... FROM numbers(30)`; after 3 s
// press Cancel, then look at system.processes from another tab.
// Expected: Cancel stays available while the server works, and stops the query.
// Actual: Cancel is disabled; the query stays in system.processes.
// Code: the run counts as finished at the "started" event; the ClickHouse HTTP stream is not
// cancelled when fetching pauses (crates/platypus-driver-clickhouse).
import assert from "node:assert/strict";
import { DB, button, clearStatus, gridTsv, openEditor, runStatement, saveConnections, setEditor, skipUnless } from "../editor-helpers.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = () => skipUnless("clickhouse");
export const setup = ({ dir }) => saveConnections(dir, ["clickhouse"]);

export default async function (page) {
  await openEditor(page, DB.clickhouse.name);
  await setEditor(page, "SELECT sleepEachRow(1) AS qa_sql_ch_cancel FROM numbers(30) SETTINGS max_block_size = 1");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.sleep(3000);
  const cancellable = await page.eval(`${button("Cancel")}.disabled === false`);
  if (cancellable) await page.eval(`${button("Cancel")}.click()`);
  await page.sleep(1500);
  await openEditor(page, DB.clickhouse.name);
  await runStatement(page, "SELECT count() AS running FROM system.processes WHERE query LIKE '%qa_sql_ch_cancel%' AND query NOT LIKE '%system.processes%'");
  const running = await gridTsv(page);
  await runStatement(page, "KILL QUERY WHERE query LIKE '%qa_sql_ch_cancel%' AND query NOT LIKE '%KILL QUERY%' ASYNC");
  assert.ok(cancellable, "Cancel is disabled while the query runs");
  assert.equal(running, "running\n0");
}
