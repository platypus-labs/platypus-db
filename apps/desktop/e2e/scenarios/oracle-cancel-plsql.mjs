// Bug ORA-15 (low): Cancel does not stop a running PL/SQL call on Oracle. A query is broken
// within a second, but "BEGIN DBMS_SESSION.SLEEP(20); END;" keeps running for the whole 20
// seconds after Cancel and only then ends as "Query cancelled". The driver's break
// (crates/platypus-driver-oracle/src/lib.rs, OracleCanceller -> Connection::break_execution)
// does not interrupt the server-side wait here; SQL*Plus' Ctrl+C stops the same call. Seen
// against the Docker container (out-of-band breaks may not cross Docker's port forwarding):
// worth checking the break mode (DISABLE_OOB) or falling back to killing the call.
// Steps: run BEGIN DBMS_SESSION.SLEEP(20); END; wait 2 s; click Cancel.
// Expected: cancelled within a few seconds. Actual: ends after the 20 s sleep.
import { assert, button, clearStatus, idle, needsOracle, oracleEditor, oracleSetup, visibleTab } from "../oracle-helpers.mjs";
import { setEditor } from "../editor-helpers.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = needsOracle();
export const setup = ({ dir }) => oracleSetup(dir);

export default async function (page) {
  await oracleEditor(page);
  await setEditor(page, "BEGIN DBMS_SESSION.SLEEP(20); END;");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${button(visibleTab, "Cancel")}?.disabled === false`, 10_000);
  await page.sleep(2000);
  const started = Date.now();
  await page.eval(`${button(visibleTab, "Cancel")}.click()`);
  await idle(page, "true", 30_000);
  const took = Date.now() - started;
  assert.ok(took < 8000, `the call ran ${took} ms after Cancel`);
}
