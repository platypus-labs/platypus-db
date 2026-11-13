// Bug ORA-05: DBMS_OUTPUT is never shown. An anonymous block that prints with
// DBMS_OUTPUT.PUT_LINE ends with "0 rows affected" and the text is lost: the driver never
// enables the buffer nor reads it back (DBMS_OUTPUT.ENABLE / GET_LINES), and the editor has
// no output panel for it. DBeaver ("Server output"), SQL Developer and SQL*Plus
// (SET SERVEROUTPUT ON) all show it; it is the usual way to debug PL/SQL.
// Steps: run DECLARE x NUMBER := 1; BEGIN DBMS_OUTPUT.PUT_LINE('hello ' || x); END;
// Expected: "hello 1" visible in the tab (an output pane or the messages). Actual: nothing.
import { assert, needsOracle, oracleEditor, oracleSetup, runOra, visibleTab } from "../oracle-helpers.mjs";

export const skip = needsOracle();
export const setup = ({ dir }) => oracleSetup(dir);

export default async function (page) {
  await oracleEditor(page);
  const { error } = await runOra(page, "DECLARE\n  x NUMBER := 1;\nBEGIN\n  DBMS_OUTPUT.PUT_LINE('qa-ora ' || 'hello ' || x);\nEND;");
  assert.equal(error, null);
  await page.sleep(500);
  // Outside the editor's own text, which holds the call.
  const shown = await page.eval(`(() => {
    const tab = ${visibleTab}.cloneNode(true);
    tab.querySelectorAll('.cm-editor').forEach(e => e.remove());
    return tab.textContent;
  })()`);
  assert.ok(shown.includes("qa-ora hello 1"), "DBMS_OUTPUT text not shown");
}
