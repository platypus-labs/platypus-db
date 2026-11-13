// Regression guard (passes): a read-only Oracle connection refuses, before running them,
// INSERT, MERGE, SELECT … FOR UPDATE, LOCK TABLE, COMMENT ON, CALL and anonymous PL/SQL blocks
// that write, and still runs queries (plain, WITH, calling a function).
import { CONN, assert, dropAll, editorText, needsOracle, ora, oracleSetup, status, visibleTab } from "../oracle-helpers.mjs";
import { CTRL } from "../cdp.mjs";
import { connectOracle } from "../oracle-helpers.mjs";
import { openEditor } from "../safety-helpers.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir, { fields: { read_only: true } });
  dropAll("TABLE QA_ORA_RO");
  ora("CREATE TABLE QA_ORA_RO (ID NUMBER PRIMARY KEY);\nINSERT INTO QA_ORA_RO VALUES (1);\nCOMMIT;");
}

const notice = `(${visibleTab}?.querySelector('.notice.warn')?.textContent ?? '')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;

async function attempt(page, sql) {
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await openEditor(page, CONN);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${tabs}`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`/read-only/.test(${notice}) || (${cancel}?.disabled === true && ${status} !== '' && !${status}.includes('Executing'))`, 30_000);
  return /read-only/.test(await page.eval(notice)) ? "refused" : await page.eval(status);
}

export default async function (page) {
  try {
    await connectOracle(page);
    for (const sql of [
      "INSERT INTO QA_ORA_RO VALUES (2)",
      "MERGE INTO QA_ORA_RO r USING DUAL ON (r.ID = 9) WHEN NOT MATCHED THEN INSERT VALUES (9)",
      "SELECT * FROM QA_ORA_RO FOR UPDATE",
      "LOCK TABLE QA_ORA_RO IN EXCLUSIVE MODE",
      "COMMENT ON TABLE QA_ORA_RO IS 'x'",
      "CALL DBMS_OUTPUT.PUT_LINE('x')",
      "BEGIN DELETE FROM QA_ORA_RO; END;",
    ]) {
      assert.equal(await attempt(page, sql), "refused", sql);
    }
    for (const sql of ["SELECT * FROM QA_ORA_RO", "WITH x AS (SELECT ID FROM QA_ORA_RO) SELECT * FROM x", "SELECT UPPER('a') FROM DUAL"]) {
      assert.equal(await attempt(page, sql), "1 row", sql);
    }
    assert.equal(ora("SELECT COUNT(*) FROM QA_ORA_RO;"), "1");
  } finally {
    dropAll("TABLE QA_ORA_RO");
  }
}
