// Bug ORA-23 (low): the production confirmation on Oracle says "(row count unknown)" for a
// DELETE / UPDATE without WHERE or a TRUNCATE on any table whose optimizer statistics were never
// gathered (every new or small table): the estimate reads only ALL_TABLES.NUM_ROWS
// (crates/platypus-sql/src/dialect.rs, row_estimate_sql), which is NULL then, and unlike
// PostgreSQL there is no fallback to a count. The number is what makes the warning useful.
// Steps: production connection; table QA_ORA_GR with 3 rows; run DELETE FROM QA_ORA_GR.
// Expected: the confirmation names 3 rows. Actual: "(row count unknown)".
import { assert, dropAll, editorText, needsOracle, ora, oracleEditor, oracleSetup, visibleTab } from "../oracle-helpers.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir, { fields: { connection_type: "prod" } });
  dropAll("TABLE QA_ORA_GR");
  ora("CREATE TABLE QA_ORA_GR (ID NUMBER PRIMARY KEY);\nINSERT INTO QA_ORA_GR SELECT LEVEL FROM DUAL CONNECT BY LEVEL <= 3;\nCOMMIT;");
}

export default async function (page) {
  try {
    await oracleEditor(page);
    const sql = "DELETE FROM QA_ORA_GR";
    await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    await page.type(sql);
    await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
    await page.key("Escape", "Escape", 27);
    await page.key("Enter", "Enter", 13, CTRL);
    await page.waitFor(`document.querySelector('.modal.guard') !== null`, 60_000);
    await page.sleep(500);
    const text = await page.eval(`document.querySelector('.modal.guard').textContent`);
    await page.eval(`[...document.querySelectorAll('.modal.guard button')].find(b => b.textContent.trim() === 'Cancel').click()`);
    assert.doesNotMatch(text, /row count unknown/, text);
    assert.match(text, /\b3 rows\b/, text);
  } finally {
    dropAll("TABLE QA_ORA_GR");
  }
}
