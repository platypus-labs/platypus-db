// Redis commands in the editor: replies as rows (a value, a map's fields, a list's items),
// quoting as redis-cli reads it, comments, scripts of several lines, MULTI/EXEC queued on
// the client, SELECT moving the session, errors from the server (unknown command,
// WRONGTYPE) and from the editor (unbalanced quotes, refused SUBSCRIBE and CLIENT REPLY),
// the confirmation before FLUSHDB, and Cancel on a blocking BLPOP.
// Skipped when Redis is not running.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import {
  button,
  clearStatus,
  gridTsv,
  idle,
  openEditor,
  resultError,
  resultTabs,
  runScript,
  runStatement,
  setEditor,
  status,
} from "../qa-sql.mjs";
import { connection, redis, saveConnections, skip as skipRedis, text } from "../redis-helpers.mjs";

const NAME = "E2E Redis editor";
const DB = 14;

export const skip = skipRedis;
export const setup = ({ dir }) => saveConnections(dir, connection(2, NAME, DB));

export default async function (page) {
  await redis(DB, [["FLUSHDB"]]);
  await openEditor(page, NAME);

  assert.equal(await runStatement(page, `SET greeting "hello world"`), "1 row");
  assert.equal(await gridTsv(page), "value\nOK");
  assert.equal(await runStatement(page, "GET greeting"), "1 row");
  assert.equal(await gridTsv(page), "value\nhello world");
  assert.equal(text((await redis(DB, [["GET", "greeting"]]))[0]), "hello world");
  assert.equal(await runStatement(page, "GET missing"), "1 row");
  assert.equal(await gridTsv(page), "value\n");

  assert.equal(await runStatement(page, "HSET user name Ada lang 'it\\'s'"), "1 row");
  assert.equal(await runStatement(page, "HGETALL user"), "2 rows");
  assert.equal(await gridTsv(page), "field\tvalue\nname\tAda\nlang\tit's");
  await runStatement(page, "RPUSH letters a b c");
  assert.equal(await runStatement(page, "LRANGE letters 0 -1"), "3 rows");
  assert.equal(await gridTsv(page), "index\tvalue\n0\ta\n1\tb\n2\tc");

  // Comments are skipped; the command under the cursor runs.
  assert.equal(await runStatement(page, "# the greeting\n-- again\nSTRLEN greeting"), "1 row");
  assert.equal(await gridTsv(page), "value\n11");

  // A script runs each line, with a result each.
  await runScript(page, "INCR counter\nINCRBY counter 9\nGET counter");
  assert.equal((await page.eval(resultTabs)).length, 3);
  assert.equal(await gridTsv(page), "value\n10");

  // MULTI queues on the client; EXEC sends the transaction and shows each reply.
  await runScript(page, "MULTI\nINCR counter\nLPUSH user x\nEXEC");
  const exec = (await gridTsv(page)).split("\n");
  assert.equal(exec[0], "index\tvalue");
  assert.equal(exec[1], "0\t11");
  assert.match(exec[2], /^1\tWRONGTYPE/);
  assert.equal(await runStatement(page, "EXEC"), "Error");
  assert.match(await page.eval(resultError), /EXEC without MULTI/);

  // SELECT moves the session; BROWSE can still name its database.
  assert.match(await runStatement(page, "SELECT 15"), /rows? affected|Done|OK|^\d/);
  assert.equal(await runStatement(page, "GET greeting"), "1 row");
  assert.equal(await gridTsv(page), "value\n");
  assert.equal(await runStatement(page, "BROWSE db14.string"), "2 rows");
  await runStatement(page, "SELECT 14");
  assert.equal(await runStatement(page, "GET greeting"), "1 row");
  assert.equal(await gridTsv(page), "value\nhello world");

  // Errors keep the server's code; the session goes on.
  assert.equal(await runStatement(page, "NOSUCHCOMMAND x"), "Error");
  assert.match(await page.eval(resultError), /ERR.*unknown command/i);
  assert.equal(await runStatement(page, "LPUSH user x"), "Error");
  assert.match(await page.eval(resultError), /WRONGTYPE/);
  assert.equal(await runStatement(page, `SET "unbalanced`), "Error");
  assert.match(await page.eval(resultError), /unbalanced quotes/);
  assert.equal(await runStatement(page, "SUBSCRIBE news"), "Error");
  assert.match(await page.eval(resultError), /not available in the editor/);
  assert.equal(await runStatement(page, "CLIENT REPLY OFF"), "Error");
  assert.match(await page.eval(resultError), /not available in the editor/);
  assert.equal(await runStatement(page, "GET greeting"), "1 row");

  // FLUSHDB asks first, as a DELETE without WHERE does; Cancel keeps the keys.
  await setEditor(page, "FLUSHDB");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.modal.guard') !== null`, 10_000);
  const guard = await page.eval(`document.querySelector('.modal.guard').textContent`);
  assert.match(guard, /FLUSHDB deletes every key of the session's database/, guard);
  assert.doesNotMatch(guard, /WHERE|row count/, guard);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal') === null`, 5_000);
  assert.equal(text((await redis(DB, [["GET", "greeting"]]))[0]), "hello world");

  // Cancel stops a blocking command.
  await setEditor(page, "BLPOP never:filled 0");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${button("Cancel")}?.disabled === false`, 10_000);
  await page.sleep(300);
  await page.eval(`${button("Cancel")}.click()`);
  await page.waitFor(`${idle} && ${status} !== '' && !${status}.includes('Executing')`, 15_000);
  assert.match(`${await page.eval(status)} ${await page.eval(resultError)}`, /cancel/i);
  assert.equal(await runStatement(page, "PING"), "1 row");
  assert.equal(await gridTsv(page), "value\nPONG");
  await redis(DB, [["FLUSHDB"]]);
}
