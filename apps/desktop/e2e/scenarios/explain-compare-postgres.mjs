// Group E against the dev PostgreSQL: a plan from the editor is refreshed and compared with
// its previous run, step by step; its graph shows the real plan, and Compare… offers no
// previous run before the first refresh.
import assert from "node:assert/strict";
import { CTRL, SHIFT } from "../cdp.mjs";
import { connections, editorText, needs, openEditor, visibleTab } from "../grid-helpers.mjs";
import { answer, askCompare, compareWith, explainTab, operations, showView } from "../plans.mjs";

const DB = "pg";
const NAME = "QA E PostgreSQL";
const SQL = "SELECT c.country, count(*) FROM demo.orders o JOIN demo.customers c ON c.id = o.customer_id WHERE o.status = 'paid' GROUP BY c.country";

export const skip = needs(DB);

export function setup({ dir }) {
  connections(dir, [[NAME, DB]]);
}

export default async function (page) {
  await openEditor(page, NAME, DB);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type(SQL);
  await page.waitFor(`${editorText} === ${JSON.stringify(SQL)}`);
  await page.key("Escape", "Escape", 27);
  await page.key("E", "KeyE", 69, CTRL | SHIFT);
  await page.waitFor(`${explainTab}?.querySelector('.explain-node') != null`, 20_000);
  const first = await page.eval(operations);
  assert.ok(first.includes("Hash Join") || first.includes("Nested Loop"), `a join: ${first}`);

  // Before a refresh there is only another plan to compare with.
  assert.deepEqual(await askCompare(page), ["Cancel", "Another plan…"]);
  await answer(page, "Cancel");
  await page.waitFor(`document.querySelector('.modal.confirm') === null`);

  // Refreshed, the plan compares with the run before: the same steps on both sides.
  await page.eval(`${explainTab}.querySelector('button[aria-label="Refresh"]').click()`);
  await page.waitFor(`${explainTab}.querySelector('button[aria-label="Refresh"]').disabled === true`, 5_000).catch(() => {});
  await page.waitFor(`${explainTab}.querySelector('button[aria-label="Refresh"]').disabled === false`, 20_000);
  await compareWith(page, "The previous run");
  await page.waitFor(`${explainTab}.querySelector('.explain-compare-table') !== null`, 10_000);
  assert.equal(await page.eval(`${explainTab}.querySelectorAll('.explain-compare-table thead th')[0].textContent`), "Previous run");
  const rows = await page.eval(`[...${explainTab}.querySelectorAll('.explain-compare-table tbody tr')].map(r => [r.className, r.children[0].textContent, r.children[1].textContent])`);
  assert.ok(rows.length >= 3, JSON.stringify(rows));
  assert.ok(rows.every(([status, before, after]) => status === "" && before === after), JSON.stringify(rows));
  // A plain explain compares on cost, which has not changed.
  assert.match(await page.eval(`${explainTab}.querySelector('.explain-compare-summary').textContent`), /^Total cost.*=/);

  // The graph of the real plan: as many boxes as steps.
  await showView(page, "Graph");
  await page.waitFor(`${explainTab}.querySelectorAll('.explain-box').length === ${first.length}`);
}
