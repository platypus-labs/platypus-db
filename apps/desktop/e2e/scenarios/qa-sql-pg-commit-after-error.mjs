// SQL-04. PostgreSQL, manual commit: after a statement fails inside the transaction, PostgreSQL
// aborts the whole transaction; pressing Commit then ends it with a ROLLBACK, and the app
// shows nothing: the "Transaction open · 1 change" badge goes away as after a real commit,
// and the change made before the error is silently lost.
// Steps: manual commit; INSERT id 1 (ok); INSERT id 1 again (duplicate key error); Commit.
// Expected: the user is told that the transaction was rolled back (or Commit is refused
// until Rollback), as the server answered ROLLBACK to the COMMIT.
// Actual: no message; the first INSERT is gone (SELECT count(*) = 0).
// Code: apps/desktop/src/components/QueryTab.tsx endTransaction(); the PG driver's commit
// ignores the ROLLBACK command tag.
import assert from "node:assert/strict";
import { DB, button, gridTsv, openEditor, runScript, runStatement, saveConnections, skipUnless, visibleTab } from "../qa-sql.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

const badge = `(${visibleTab}.querySelector('.tx-open')?.textContent ?? null)`;
/** The page's text outside the editor and the result grid. */
const messages = `[...document.querySelectorAll('.toast, .notice, .result-error, .result-message, .modal')].map(e => e.textContent).join(' | ')`;

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await runScript(page, "DROP TABLE IF EXISTS qa_sql_commit_err;\nCREATE TABLE qa_sql_commit_err (id int PRIMARY KEY);");
  await page.eval(`${button("Auto-commit")}.click()`);
  await page.waitFor(`${button("Manual commit")} !== undefined`);
  await runStatement(page, "INSERT INTO qa_sql_commit_err VALUES (1)");
  await page.waitFor(`${badge}?.includes('1 change')`);
  assert.equal(await runStatement(page, "INSERT INTO qa_sql_commit_err VALUES (1)"), "Error");
  await page.eval(`${button("Commit")}.click()`);
  await page.waitFor(`${badge} === null`, 10_000);
  await page.sleep(500);
  const shown = await page.eval(messages);
  await runStatement(page, "SELECT count(*) AS n FROM qa_sql_commit_err");
  const rows = await gridTsv(page);
  await page.eval(`${button("Manual commit")}.click()`);
  await runStatement(page, "DROP TABLE IF EXISTS qa_sql_commit_err");
  assert.ok(rows === "n\n1" || /roll(ed)? ?back/i.test(shown), `the commit rolled back without a word: rows ${JSON.stringify(rows)}, messages ${JSON.stringify(shown)}`);
}
