// SQL-16. Messages from the server are not shown anywhere: PostgreSQL RAISE NOTICE, SQL
// Server PRINT and RAISERROR(..., 0, 1), MySQL warnings (e.g. "Truncated incorrect INTEGER
// value"). Debugging procedures needs them; DBeaver shows them in its Output panel.
// Steps: PostgreSQL, Ctrl+Enter on `DO $$ BEGIN RAISE NOTICE 'qa_sql notice text'; END $$`.
// Expected: the notice text appears in the result area (or an output/messages panel).
// Actual: only "0 rows affected".
// Code: the drivers drop notices (tokio-postgres notices, tiberius info tokens); there is no
// message channel from QueryEvent to the result panel.
import assert from "node:assert/strict";
import { DB, openEditor, runStatement, saveConnections, skipUnless, visibleTab } from "../qa-sql.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

/** Text of the page outside the editor. */
const outside = `(() => { const c = ${visibleTab}.querySelector('.cm-editor'); const parts = []; const walk = (n) => { if (n === c) return; if (n.nodeType === 3) parts.push(n.textContent); n.childNodes.forEach(walk); }; walk(document.body); return parts.join(' '); })()`;

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await runStatement(page, "DO $$ BEGIN RAISE NOTICE 'qa_sql notice text'; END $$");
  await page.sleep(500);
  assert.match(await page.eval(outside), /qa_sql notice text/);
}
