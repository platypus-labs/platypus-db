// SQL-05. Closing an editor tab while its statement runs does not stop the statement on the
// server: it runs to its end (a long UPDATE would still commit), holding the connection and
// its locks. The tab closes at once with no question.
// Steps: on PostgreSQL run `SELECT pg_sleep(15)` (application_name set to find it), close
// the tab while it runs, look at pg_stat_activity from another tab.
// Expected: the statement is cancelled when its tab closes (or the app asks first).
// Actual: it is still active seconds later.
// Code: apps/desktop/src/components/QueryTab.tsx unmount effect: closeJobs() calls
// api.closeQuery, which drops the job (commands/query.rs close_query) without cancelling it.
import assert from "node:assert/strict";
import { DB, button, gridTsv, openEditor, runStatement, saveConnections, setEditor, skipUnless } from "../qa-sql.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await runStatement(page, "SET application_name = 'qa_sql_close_running'");
  await setEditor(page, "SELECT pg_sleep(15) AS slept");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${button("Cancel")}?.disabled === false`, 10_000);
  await page.sleep(1000);
  await page.eval(`document.querySelector('.tab.active button[aria-label="Close tab"]').click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length === 0 || document.querySelector('.modal') !== null`);
  // A question before closing would do too.
  if (await page.eval(`document.querySelector('.modal') !== null`)) return;
  await page.sleep(3000);
  await openEditor(page, DB.pg.name);
  await runStatement(page, "SELECT count(*) AS active FROM pg_stat_activity WHERE application_name = 'qa_sql_close_running' AND state = 'active'");
  assert.equal(await gridTsv(page), "active\n0", "the statement of the closed tab still runs");
}
