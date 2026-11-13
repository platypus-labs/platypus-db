// Regression guard (passes): PostgreSQL transactions from the editor. Manual commit keeps an
// INSERT invisible to another tab until Commit, Rollback undoes one, a typed BEGIN in
// auto-commit shows the open-transaction badge, closing a tab with an open transaction asks
// first, and a session killed inside a transaction says that the transaction was rolled back.
import assert from "node:assert/strict";
import { DB, button, gridTsv, openEditor, resultError, runScript, runStatement, saveConnections, skipUnless, visibleTab } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

const badge = `(${visibleTab}.querySelector('.tx-open')?.textContent ?? null)`;
const tabs = `[...document.querySelectorAll('.tab')]`;

/** Shows the editor tab number `i` (0-based) and waits for it. */
async function show(page, i) {
  await page.eval(`${tabs}[${i}].click()`);
  await page.waitFor(`${tabs}[${i}].classList.contains('active')`);
}

async function count(page) {
  await runStatement(page, "SELECT count(*) AS n FROM qa_sql_ok_tx");
  return gridTsv(page);
}

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await runScript(page, "DROP TABLE IF EXISTS qa_sql_ok_tx;\nCREATE TABLE qa_sql_ok_tx (id int PRIMARY KEY);");
  await openEditor(page, DB.pg.name);
  await show(page, 0);
  await page.eval(`${button("Auto-commit")}.click()`);
  await page.waitFor(`${button("Manual commit")} !== undefined`);
  await runStatement(page, "INSERT INTO qa_sql_ok_tx VALUES (1)");
  await page.waitFor(`${badge}?.includes('1 change')`);
  await show(page, 1);
  assert.equal(await count(page), "n\n0", "uncommitted row visible elsewhere");
  await show(page, 0);
  await page.eval(`${button("Rollback")}.click()`);
  await page.waitFor(`${badge} === null`);
  await runStatement(page, "INSERT INTO qa_sql_ok_tx VALUES (2)");
  await page.eval(`${button("Commit")}.click()`);
  await page.waitFor(`${badge} === null`);
  await show(page, 1);
  assert.equal(await count(page), "n\n1");

  // A typed BEGIN in auto-commit mode.
  await runScript(page, "BEGIN;\nINSERT INTO qa_sql_ok_tx VALUES (3);");
  await page.waitFor(`${badge}?.includes('Transaction open')`);
  await runStatement(page, "ROLLBACK");
  await page.waitFor(`${badge} === null`);

  // The session dies inside a transaction.
  await show(page, 0);
  await runStatement(page, "INSERT INTO qa_sql_ok_tx VALUES (4)");
  await runStatement(page, "SELECT pg_terminate_backend(pg_backend_pid())").catch(() => null);
  await runStatement(page, "INSERT INTO qa_sql_ok_tx VALUES (5)");
  assert.match((await page.eval(resultError)) ?? "", /transaction was rolled back/);

  // Closing a tab with an open transaction asks first: Commit or Rollback, then close.
  await runStatement(page, "INSERT INTO qa_sql_ok_tx VALUES (6)");
  await page.waitFor(`${badge}?.includes('change')`);
  await page.eval(`document.querySelector('.tab.active button[aria-label="Close tab"]').click()`);
  await page.waitFor(`document.querySelector('.modal')?.textContent.includes('open transaction')`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Rollback').click()`);
  await page.waitFor(`${tabs}.length === 1`);
  assert.equal(await count(page), "n\n1");
  await runStatement(page, "DROP TABLE qa_sql_ok_tx");
}
