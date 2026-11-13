// Bug ORA-13: Explain plan on Oracle shows no plan. The editor's context menu offers
// "Explain plan" on every SQL connection, but the Oracle driver has no explain
// (crates/platypus-driver-oracle/src/lib.rs, capabilities without `explain`, no
// Connection::explain), so the explain tab shows an "unsupported" message, although
// EXPLAIN PLAN FOR + DBMS_XPLAN.DISPLAY works from the editor (DBeaver shows the plan tree).
// Steps: SELECT * FROM QA_ORA_XP WHERE ID = 1 in the editor; context menu › Explain plan.
// Expected: a plan naming the table access (INDEX UNIQUE SCAN on the key, or TABLE ACCESS) or
// no Explain entry. Actual: "unsupported…".
import { assert, dropAll, needsOracle, ora, oracleEditor, oracleSetup } from "../oracle-helpers.mjs";
import { editorMenu, setEditor } from "../editor-helpers.mjs";

export const skip = needsOracle();
const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_XP");
  ora("CREATE TABLE QA_ORA_XP (ID NUMBER PRIMARY KEY, V VARCHAR2(10));");
}

export default async function (page) {
  try {
    await oracleEditor(page);
    await setEditor(page, "SELECT * FROM QA_ORA_XP WHERE ID = 1");
    await editorMenu(page, "Explain plan");
    await page.waitFor(`${explainTab} != null && !${explainTab}.textContent.includes('Running') && !${explainTab}.textContent.includes('Loading')`, 30_000);
    await page.sleep(500);
    const text = await page.eval(`${explainTab}.textContent`);
    assert.doesNotMatch(text, /unsupported/i, text.slice(0, 300));
    assert.match(text, /INDEX UNIQUE SCAN|TABLE ACCESS/i, text.slice(0, 300));
  } finally {
    dropAll("TABLE QA_ORA_XP");
  }
}
