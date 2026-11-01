// Redis from docker/compose.yml, browsed: the navigator lists the database's key types as
// tables (without the SQL-only actions), the data viewer reads each type with BROWSE (rows of
// string, hash, list, set, zset, JSON and stream keys), the document panel unfolds a JSON
// value, MATCH filters the keys, LIMIT caps the rows, a big list arrives in pages, a value that
// is not UTF-8 shows escaped, and an unknown type is an error.
// Skipped when Redis is not running.
import assert from "node:assert/strict";
import { submenuItems } from "../menu-sub.mjs";
import { gridTsv, openEditor, resultError, runStatement, status, treeRow, visibleTab, editorText } from "../qa-sql.mjs";
import { connection, redis, saveConnections, skip as skipRedis } from "../redis-helpers.mjs";

const NAME = "E2E Redis";
const DB = 13;

export const skip = skipRedis;
export const setup = ({ dir }) => saveConnections(dir, connection(1, NAME, DB));

async function seed() {
  const big = Array.from({ length: 3000 }, (_, i) => `item${i}`);
  await redis(DB, [
    ["FLUSHDB"],
    ["SET", "user:1", "alice"],
    ["SET", "user:2", "bob", "EX", "500"],
    ["SET", "other", "x"],
    ["SET", "bin", Buffer.from([0xff, 0x00, 0x41])],
    ["SET", "", "empty key"],
    ["HSET", "h:1", "name", "Ada", "lang", "en"],
    ["RPUSH", "big", ...big],
    ["SADD", "tags", "red", "green"],
    ["ZADD", "board", "1.5", "alice", "3", "bob"],
    ["JSON.SET", "doc", "$", JSON.stringify({ a: { b: [1, 2] }, c: "text" })],
    ["XADD", "events", "1-1", "kind", "click"],
  ]);
}

async function menu(page, label) {
  await page.eval(`${treeRow(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  return page.eval(`[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`);
}

const lines = (tsv) => tsv.split("\n");

export default async function (page) {
  await seed();
  await page.waitFor(`${treeRow(NAME)} !== undefined`, 30_000);
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  // The session's database opens by itself, with a table per key type.
  await page.waitFor(`${treeRow("db13")} !== undefined && ${treeRow("stream")} !== undefined`, 20_000);
  for (const type of ["string", "hash", "list", "set", "zset", "json", "stream"]) {
    assert.ok(await page.eval(`${treeRow(type)} !== undefined`), `${type} in the navigator`);
  }

  // Only what applies to a key type: no SQL builder, DDL, INSERT or TRUNCATE.
  const items = await menu(page, "hash");
  for (const item of ["Open data", "Generate SQL"]) assert.ok(items.includes(item), `${item} in ${items}`);
  for (const item of ["Visual query builder", "Generate DDL", "Generate INSERT", "Truncate…", "Edit table…"]) {
    assert.ok(!items.includes(item), `no ${item} in ${items}`);
  }
  // ...and neither does the Generate SQL submenu.
  const sql = await submenuItems(page, "Generate SQL");
  for (const item of ["Generate DDL", "Generate INSERT"]) assert.ok(!sql.includes(item), `no ${item} in ${sql}`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent === 'Open data').click()`);
  await page.waitFor(`${editorText} === 'BROWSE db13.hash'`, 15_000);
  await page.waitFor(`${status} === '2 rows'`, 15_000);
  assert.deepEqual(lines(await gridTsv(page)).sort(), ["h:1\tlang\ten", "h:1\tname\tAda", "key\tfield\tvalue"]);

  // The editor reads every type.
  await openEditor(page, NAME);
  assert.equal(await runStatement(page, "BROWSE string"), "5 rows");
  const strings = lines(await gridTsv(page));
  assert.equal(strings[0], "key\tvalue\tttl");
  // The copy quotes a cell with quotes in it, as CSV does.
  assert.ok(strings.includes('bin\t"""\\xff\\x00A"""\t'), strings.join("|"));
  assert.ok(strings.includes("\tempty key\t"), strings.join("|"));
  const ttl = Number(strings.find((l) => l.startsWith("user:2\t")).split("\t")[2]);
  assert.ok(ttl > 0 && ttl <= 500, `ttl ${ttl}`);

  // MATCH filters the keys, LIMIT caps the rows.
  assert.equal(await runStatement(page, "BROWSE string MATCH user:*"), "2 rows");
  assert.equal(await runStatement(page, "BROWSE db13.string MATCH 'user:?' LIMIT 1"), "1 row");
  assert.equal(await runStatement(page, "BROWSE string MATCH nothing*"), "0 rows");

  // A big list arrives in pages: the first one, then every item in order with "Fetch all".
  assert.match(await runStatement(page, "BROWSE list", 60_000), /^1,000 rows loaded · more available/);
  await page.eval(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === 'Fetch all').click()`);
  await page.waitFor(`/^3,000 rows/.test(${status})`, 30_000);
  const all = lines(await gridTsv(page));
  assert.equal(all.length, 3001);
  assert.equal(all[3000], "big\t2999\titem2999");
  assert.equal(await runStatement(page, "BROWSE list LIMIT 10"), "10 rows");
  assert.deepEqual(lines(await gridTsv(page)).slice(0, 3), ["key\tindex\tvalue", "big\t0\titem0", "big\t1\titem1"]);

  assert.equal(await runStatement(page, "BROWSE set"), "2 rows");
  assert.deepEqual(lines(await gridTsv(page)).sort(), ["key\tmember", "tags\tgreen", "tags\tred"]);
  assert.equal(await runStatement(page, "BROWSE zset"), "2 rows");
  assert.deepEqual(lines(await gridTsv(page)), ["key\tmember\tscore", "board\talice\t1.5", "board\tbob\t3.0"]);
  assert.equal(await runStatement(page, "BROWSE stream"), "1 row");
  assert.deepEqual(lines(await gridTsv(page)), ["key\tid\tfields", 'events\t1-1\t"{""kind"":""click""}"']);

  // A JSON value unfolds in the document panel.
  assert.equal(await runStatement(page, "BROWSE json"), "1 row");
  const grid = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  await page.click(grid.x + 60, grid.y + 40);
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.document-panel .doc-key')].some(k => k.textContent === 'b')`, 10_000);

  // Another database by name: empty here.
  assert.equal(await runStatement(page, "BROWSE db12.hash"), "0 rows");
  assert.equal(await runStatement(page, "BROWSE nope"), "Error");
  assert.match(await page.eval(resultError), /unknown type `nope`/);
  assert.equal(await runStatement(page, "BROWSE hash LIMIT many"), "Error");
  assert.match(await page.eval(resultError), /invalid LIMIT/);
  await redis(DB, [["FLUSHDB"]]);
}
