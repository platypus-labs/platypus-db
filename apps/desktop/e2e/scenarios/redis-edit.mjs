// Editing Redis keys in the grid: the preview shows the commands (SELECT, then HSET, SET …
// KEEPTTL, EXPIRE, LSET/LREM, SADD, ZADD), saving runs them as one MULTI/EXEC and reloads,
// the undo script puts things back; a key that is not UTF-8 keeps its bytes, a deleted list
// item goes by its index, a new set member is added, and a bad score or a missing key is
// refused with a message.
// Skipped when Redis is not running.
import assert from "node:assert/strict";
import { treeRow, visibleTab, status, editorText } from "../qa-sql.mjs";
import { connection, redis, saveConnections, skip as skipRedis, text } from "../redis-helpers.mjs";

const NAME = "E2E Redis edit";
const DB = 15;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;

export const skip = skipRedis;
export const setup = ({ dir }) => saveConnections(dir, connection(3, NAME, DB));

async function openTable(page, name, rows) {
  await page.waitFor(`${treeRow(name)} !== undefined`, 20_000);
  await page.eval(`${treeRow(name)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${editorText} === 'BROWSE db15.${name}'`, 15_000);
  await page.waitFor(`${status} === '${rows}' && ${editBar}.includes('to edit a cell')`, 15_000);
}

/** Moves to the cell at `row`, `col` (from the top-left one) and types `value` into it. */
async function editCell(page, row, col, value) {
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, 2);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.eval(`(() => { const e = document.querySelector('.cell-editor'); e.select?.(); })()`);
  await page.key("a", "KeyA", 65, 2);
  if (value) await page.type(value);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
}

async function preview(page) {
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Preview SQL"]').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null || /needs|cannot|invalid/.test(${editBar})`, 10_000);
  return page.eval(`document.querySelector('.sql-preview')?.textContent ?? null`);
}

async function save(page, rows) {
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  await page.waitFor(`${editBar}.includes('to edit a cell') && ${status} === '${rows}'`, 15_000);
}

/** The error the app shows for a refused save or preview. */
const shownError = `[...document.querySelectorAll('.toast, .notice, .error, .edit-bar')].map(e => e.textContent).join(' | ')`;

export default async function (page) {
  await redis(DB, [
    ["FLUSHDB"],
    ["HSET", "h", "f1", "v1", "f2", "v2"],
    ["SET", "s1", "a"],
    ["SET", Buffer.from([0xff, 0x01]), "binary key"],
    ["RPUSH", "l", "a", "b", "c"],
    ["SADD", "members", "m1"],
    ["ZADD", "z", "1", "m"],
  ]);
  await page.waitFor(`${treeRow(NAME)} !== undefined`, 30_000);
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);

  // A hash field's value: HSET, after the database.
  await openTable(page, "hash", "2 rows");
  await editCell(page, 0, 2, "V1 changed");
  assert.match(await page.eval(editBar), /1 row changed/);
  assert.equal(await preview(page), 'SELECT 15\nHSET h f1 "V1 changed"');
  await save(page, "2 rows");
  assert.equal(text((await redis(DB, [["HGET", "h", "f1"]]))[0]), "V1 changed");
  // The undo script is Redis commands too.
  await page.waitFor(`[...${visibleTab}.querySelectorAll('button')].some(b => b.textContent.includes('Undo script'))`, 10_000);

  // Strings: a value keeps its TTL (KEEPTTL), a TTL is set; the binary key keeps its bytes.
  await openTable(page, "string", "2 rows");
  // Rows in the keys' byte order: the binary key (0xff…) comes last.
  await editCell(page, 0, 2, "120");
  await editCell(page, 1, 1, "still binary");
  const commands = await preview(page);
  assert.equal(commands, 'SELECT 15\nEXPIRE s1 120\nSET "\\xff\\x01" "still binary" KEEPTTL');
  await save(page, "2 rows");
  const [binary, ttl] = await redis(DB, [["GET", Buffer.from([0xff, 0x01])], ["TTL", "s1"]]);
  assert.equal(text(binary), "still binary");
  assert.ok(ttl > 0 && ttl <= 120, `ttl ${ttl}`);

  // A list item deleted by its index: marked, then removed with LREM.
  await openTable(page, "list", "3 rows");
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, 2);
  await page.key("Delete", "Delete", 46);
  const listCommands = await preview(page);
  assert.match(listCommands, /^SELECT 15\nLSET l 0 __platypus_deleted_\S+\nLREM l 0 __platypus_deleted_\S+$/);
  await save(page, "2 rows");
  assert.deepEqual((await redis(DB, [["LRANGE", "l", "0", "-1"]]))[0].map(text), ["b", "c"]);

  // A new set member.
  await openTable(page, "set", "1 row");
  await page.eval(`${grid}.focus()`);
  await page.key("Insert", "Insert", 45);
  // A new row goes on top.
  await page.waitFor(`${editBar}.includes('1 row changed')`, 10_000);
  await editCell(page, 0, 0, "members");
  await editCell(page, 0, 1, "m2");
  assert.equal(await preview(page), "SELECT 15\nSADD members m2");
  await save(page, "2 rows");
  assert.deepEqual((await redis(DB, [["SMEMBERS", "members"]]))[0].map(text).sort(), ["m1", "m2"]);

  // A score that is not a number is refused before anything runs.
  await openTable(page, "zset", "1 row");
  await editCell(page, 0, 2, "abc");
  assert.equal(await preview(page), null);
  await page.waitFor(`${shownError}.includes('invalid score')`, 10_000);
  assert.equal(text((await redis(DB, [["ZSCORE", "z", "m"]]))[0]), "1");
  await redis(DB, [["FLUSHDB"]]);
}
