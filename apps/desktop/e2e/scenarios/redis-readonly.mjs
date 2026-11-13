// A Redis connection saved as read-only: the editor runs commands that read (GET, HGETALL,
// BROWSE, SELECT) and refuses those that write (SET, DEL, FLUSHDB, CONFIG SET, an unknown
// command) before they run; the data viewer's grid is read-only. The keys stay as they were.
// Skipped when Redis is not running.
import assert from "node:assert/strict";
import { gridTsv, runStatement, treeRow, visibleTab } from "../editor-helpers.mjs";
import { openEditor, runOrGuard } from "../safety-helpers.mjs";
import { connection, redis, saveConnections, skip as skipRedis, text } from "../redis-helpers.mjs";

const NAME = "E2E Redis read-only";
const DB = 12;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;

export const skip = skipRedis;
export const setup = ({ dir }) => saveConnections(dir, connection(4, NAME, DB, "read_only = true\n"));

export default async function (page) {
  await redis(DB, [["FLUSHDB"], ["SET", "k", "v"], ["HSET", "h", "f", "1"]]);
  await openEditor(page, NAME);
  for (const command of ["SET k changed", "DEL k", "FLUSHDB", "CONFIG SET maxmemory 1", "NOSUCHCOMMAND k"]) {
    const r = await runOrGuard(page, command, { readOnly: true });
    assert.match(r, /^read-only: .*read-only/, `${command}: ${r}`);
  }
  for (const [command, rows] of [["GET k", "1 row"], ["HGETALL h", "1 row"], ["BROWSE string", "1 row"], ["TTL k", "1 row"]]) {
    assert.equal(await runStatement(page, command), rows, command);
  }
  assert.equal(await gridTsv(page), "value\n-1");
  assert.deepEqual((await redis(DB, [["GET", "k"], ["DBSIZE"]])).map(text), ["v", 2]);

  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("hash")} !== undefined`, 20_000);
  await page.eval(`${treeRow("hash")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${editBar}.startsWith('Read-only')`, 15_000);
  await redis(DB, [["FLUSHDB"]]);
}
