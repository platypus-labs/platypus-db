// SQL-25 (low). Completion after `schema.` plus a few letters mixes in keywords that merely
// contain the letters: `FROM demo.ord` lists orders, then ORDER, ORDERING, ORDINALITY,
// PASSWORD, ACCORDING… A keyword cannot follow `schema.`. (Ctrl+Space right after the dot
// lists only the schema's tables, as it should.)
// Steps: PostgreSQL, type `SELECT * FROM demo.ord`.
// Expected: only the objects of schema demo (orders).
// Actual: orders plus a dozen keywords.
// Code: apps/desktop/src/editor/completion.ts: the keyword source does not check for a
// qualifier before the word.
import assert from "node:assert/strict";
import { DB, openEditor, saveConnections, setEditor, skipUnless, visibleTab } from "../qa-sql.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

const options = `[...${visibleTab}.querySelectorAll('.cm-tooltip-autocomplete li')].map(li => li.textContent)`;

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await setEditor(page, "SELECT * FROM ");
  for (const ch of "demo.ord") {
    await page.type(ch);
    await page.sleep(30);
  }
  await page.waitFor(`${options}.includes('orders')`, 15_000);
  await page.sleep(800);
  const listed = await page.eval(options);
  assert.deepEqual(listed, ["orders"]);
}
