// Group E without a database: plans pasted in Open query plan… (from the palette and the
// navigator's menu), recognized or of the database chosen, refused when they are no plan,
// shown as a tree and as a graph, and compared with another pasted plan, step by step.
import assert from "node:assert/strict";
import { explainTab, operations, palette, pastePlan, PG_AFTER, PG_BEFORE, compareWith, showView } from "../plans.mjs";

export { setup } from "./smoke-sqlite.mjs";

const SQLITE = "QUERY PLAN\n|--SCAN a\n`--CORRELATED SCALAR SUBQUERY 1\n   `--SEARCH b USING INDEX i (x=?)";
const SQL_SERVER = "StmtText\n--------\n  |--Nested Loops(Inner Join, OUTER REFERENCES:([o].[customer_id]))\n       |--Clustered Index Scan(OBJECT:([shop].[dbo].[orders].[PK_orders] AS [o]))\n       |--Clustered Index Seek(OBJECT:([shop].[dbo].[customers].[PK_customers] AS [c]))";

const dialogError = `document.querySelector('.plan-dialog .notice.error')?.textContent ?? ''`;

export default async function (page) {
  // Nothing to open while the box is empty.
  await palette(page, "Open query plan");
  await page.waitFor(`document.querySelector('.plan-dialog') !== null`);
  assert.equal(await page.eval(`document.querySelector('.plan-dialog button[type=submit]').disabled`), true);

  // Text that is no plan, and a plan read as another database's, are refused in the dialog.
  await pastePlan(page, "hello, this is not a plan", "mysql");
  await page.waitFor(`${dialogError}.includes('no plan')`);
  await pastePlan(page, PG_BEFORE, "sqlserver");
  await page.waitFor(`${dialogError}.includes('no plan')`);
  assert.equal(await page.eval(`document.querySelector('.plan-dialog') !== null`), true, "the dialog stays open");

  // Recognized: PostgreSQL's text, with psql's frame, analyzed, with its index suggestion.
  await pastePlan(page, PG_BEFORE);
  await page.waitFor(`${explainTab} !== undefined`, 10_000);
  assert.equal(await page.eval(`document.querySelector('.plan-dialog') === null`), true);
  assert.equal(await page.eval(`${explainTab}.querySelector('.object-title strong').textContent`), "Query plan");
  assert.equal(await page.eval(`${explainTab}.querySelector('.explain-sql').textContent`), "Pasted plan");
  assert.deepEqual(await page.eval(operations), ["HashAggregate", "Hash Join", "Seq Scan", "Hash", "Seq Scan"]);
  assert.match(await page.eval(`${explainTab}.querySelector('.explain-suggestion')?.textContent ?? ''`), /^orders is read in full \(20[,.]000 rows\)/);
  // A read plan runs nothing: no Refresh.
  assert.equal(await page.eval(`${explainTab}.querySelector('button[aria-label="Refresh"]')`), null);
  // The statement's times are the plan's, at the top.
  assert.match(await page.eval(`${explainTab}.querySelector('.explain-properties').textContent`), /Execution Time16\.840 ms/);

  // The graph: a box per step, the busiest tinted; a click selects a step.
  await showView(page, "Graph");
  await page.waitFor(`${explainTab}.querySelectorAll('.explain-box').length === 5`);
  const heats = await page.eval(`[...${explainTab}.querySelectorAll('.explain-box')].map(b => parseInt(b.style.getPropertyValue('--heat')))`);
  assert.ok(Math.max(...heats) >= 50, `the full scan dominates: ${heats}`);
  await page.eval(`[...${explainTab}.querySelectorAll('.explain-box')].find(b => b.querySelector('.explain-box-detail').textContent === 'on orders o').dispatchEvent(new MouseEvent('click', { bubbles: true }))`);
  await page.waitFor(`${explainTab}.querySelector('.explain-box.selected .explain-box-op')?.textContent === 'Seq Scan'`);
  assert.match(await page.eval(`${explainTab}.querySelector('.explain-properties').textContent`), /Filter\(status = 'paid'::text\)/);
  // Zoom levels resize the drawing.
  const width = (zoom) =>
    page.eval(`(() => { [...${explainTab}.querySelectorAll('.explain-graph-zoom button')].find(b => b.textContent === ${JSON.stringify(zoom)}).click(); return 0; })()`);
  await width("50%");
  const half = await page.eval(`Number(${explainTab}.querySelector('.explain-graph svg').getAttribute('width'))`);
  await width("100%");
  const full = await page.eval(`Number(${explainTab}.querySelector('.explain-graph svg').getAttribute('width'))`);
  assert.equal(Math.round(full / half), 2);

  // Compared with another plan, which comes first: the one after the index, so this tab's
  // full scan reads as the change.
  await compareWith(page, "Another plan…");
  await page.waitFor(`document.querySelector('.plan-dialog h2')?.textContent === 'Plan to compare with'`);
  await pastePlan(page, PG_AFTER);
  await page.waitFor(`${explainTab}.querySelector('.explain-compare') !== null`, 10_000);
  assert.match(await page.eval(`${explainTab}.querySelector('.explain-compare-summary').textContent`), /Total time2\.4\d ms → 16\.75 ms\+58\d/);
  assert.equal(await page.eval(`${explainTab}.querySelector('.explain-compare-summary .explain-change').className`), "explain-change worse");
  const rows = await page.eval(`[...${explainTab}.querySelectorAll('.explain-compare-table tbody tr')].map(r => [r.className, r.children[0].textContent, r.children[1].textContent])`);
  assert.deepEqual(rows, [
    ["", "HashAggregate", "HashAggregate"],
    ["", "Hash Join", "Hash Join"],
    ["changed", "Bitmap Heap Scan on orders o", "Seq Scan on orders o"],
    ["removed", "Bitmap Index Scan on orders_status", ""],
    ["", "Hash", "Hash"],
    ["", "Seq Scan on customers c", "Seq Scan on customers c"],
  ]);
  assert.equal(await page.eval(`${explainTab}.querySelectorAll('.explain-compare-table thead th')[0].textContent`), "Pasted plan");

  // Stopping the comparison takes its view away.
  await page.eval(`[...${explainTab}.querySelectorAll('button')].find(b => b.textContent === 'Stop comparing').click()`);
  await page.waitFor(`![...${explainTab}.querySelectorAll('.object-sections button')].some(b => b.textContent === 'Comparison')`);

  // SQLite's and SQL Server's plans are recognized too, each in a tab of its own.
  await palette(page, "Open query plan");
  await pastePlan(page, SQLITE);
  await page.waitFor(`${explainTab} !== undefined && ${operations}[0] === 'QUERY PLAN'`, 10_000);
  assert.deepEqual(await page.eval(operations), ["QUERY PLAN", "SCAN a", "CORRELATED SCALAR SUBQUERY 1", "SEARCH b USING INDEX i (x=?)"]);
  await palette(page, "Open query plan");
  await pastePlan(page, SQL_SERVER);
  await page.waitFor(`${explainTab} !== undefined && ${operations}[0]?.startsWith('Nested Loops')`, 10_000);
  assert.equal((await page.eval(operations)).length, 3);
  assert.equal(await page.eval(`document.querySelectorAll('.explain-tab').length`), 3);
}
