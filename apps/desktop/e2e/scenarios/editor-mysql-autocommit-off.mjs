// qa4-sql-2 (medium). In MySQL a script that says `SET autocommit = 0` opens a transaction with
// the next DML, but the editor does not notice: no "Transaction open" badge, and closing the tab
// does not ask to Commit or Rollback, so the changes are lost without a word (the same editor
// does warn for `BEGIN` / `START TRANSACTION`).
// Steps: MySQL editor, run `SET autocommit = 0`, then `INSERT INTO qa4_ac VALUES (1)`, look at
//   the toolbar badge, click Close tab.
// Expected: the badge shows an open transaction (or at least closing the tab asks first).
// Actual: no badge; the tab closes at once and the row is rolled back.
// Code: the transaction state of the session is only followed for BEGIN/START TRANSACTION
//   (crates/platypus-driver-mysql: the in-transaction flag of the server status is not read).
import assert from "node:assert/strict";
import { DB, openEditor, runScript, runStatement, saveConnections, skipUnless, visibleTab } from "../editor-helpers.mjs";

export const skip = () => skipUnless("mysql");
export const setup = ({ dir }) => saveConnections(dir, ["mysql"]);

export default async function (page) {
  await openEditor(page, DB.mysql.name);
  await runScript(page, "DROP TABLE IF EXISTS qa4_ac;\nCREATE TABLE qa4_ac (id int);");
  try {
    await runStatement(page, "SET autocommit = 0");
    await runStatement(page, "INSERT INTO qa4_ac VALUES (1)");
    await page.sleep(500);
    const badge = await page.eval(`(${visibleTab}.querySelector('.tx-open')?.textContent ?? null)`);
    await page.eval(`document.querySelector('.tab.active button[aria-label="Close tab"]').click()`);
    const asked = await page.waitFor(`document.querySelector('.modal')?.textContent.includes('transaction')`, 3000).then(() => true, () => false);
    assert.ok(badge !== null || asked, "an open transaction (autocommit off) is shown, or closing the tab asks about it");
  } finally {
    await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Rollback')?.click()`).catch(() => {});
  }
}
