// Group E against the dev PostgreSQL from docker/compose.yml: explain analyze with the
// heatmap of each step's own time and an index suggestion. Skipped when it is not running.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";

export { setup, skip } from "./dev-postgres.mjs";

const NAME = "E2E PG";
const queryTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;
const row = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;

export default async function (page) {
  await page.waitFor(`${row} !== undefined`);
  // Expanding the connection asks for the password; a double-click then opens an editor.
  await page.eval(`${row}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`document.querySelector('.modal') === null`, 15_000);
  await page.eval(`${row}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.waitFor(`${queryTab} !== undefined`, 15_000);

  const sql = "SELECT c.country, count(*) FROM demo.orders o JOIN demo.customers c ON c.id = o.customer_id WHERE o.status = 'paid' GROUP BY c.country";
  await page.eval(`${queryTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`[...${queryTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);

  // The visible editor box: a long line scrolls the content sideways.
  await page.eval(`${queryTab}.querySelector('.cm-editor').id = 'e2e-editor'`);
  const r = await page.rect("#e2e-editor");
  await page.click(r.x + r.w / 2, r.y + 10, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Explain analyze')).click()`);
  await page.waitFor(`${explainTab}?.querySelector('.explain-share') != null`, 15_000);

  // Every step shows its share of the time; together they make the whole.
  const shares = await page.eval(`[...${explainTab}.querySelectorAll('.explain-share')].map(s => parseFloat(s.textContent))`);
  assert.equal(shares.length, await page.eval(`${explainTab}.querySelectorAll('.explain-node').length`));
  const total = shares.reduce((a, b) => a + b, 0);
  assert.ok(total > 95 && total < 105, `shares add up to ${total}`);
  // The busiest step is tinted.
  const heats = await page.eval(`[...${explainTab}.querySelectorAll('.explain-node')].map(n => n.style.getPropertyValue('--heat'))`);
  assert.ok(heats.some((h) => parseInt(h) > 0), `heat in ${heats}`);
  // orders is read in full for the status filter: an index is suggested.
  assert.match(await page.eval(`${explainTab}.querySelector(".explain-suggestion")?.textContent ?? ""`), /^orders is read in full/);
}
