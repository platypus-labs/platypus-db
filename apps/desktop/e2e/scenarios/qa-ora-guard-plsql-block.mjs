// Bug ORA-22: on a production Oracle connection the confirmation before changes does not open
// for an anonymous PL/SQL block. "DELETE FROM t" asks first, but "BEGIN DELETE FROM t; END;"
// and "BEGIN EXECUTE IMMEDIATE 'DROP TABLE t'; END;" run straight away (the delete leaves an
// uncommitted change holding a lock, the drop then fails only because of that lock). PL/SQL
// blocks are how most Oracle maintenance is run, so the guard is easy to bypass by accident.
// (The read-only connection check does refuse these blocks; the production guard's statement
// analysis treats a BEGIN … END block as neither DML nor DDL.)
// Steps: a connection with connection_type = "prod"; in the editor run
// BEGIN DELETE FROM QA_ORA_GB; END;
// Expected: the "Changes on … (production)" confirmation. Actual: it runs, "0 rows affected".
import { assert, dropAll, editorText, needsOracle, ora, oracleEditor, oracleSetup, status, visibleTab } from "../qa-ora.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir, { fields: { connection_type: "prod" } });
  dropAll("TABLE QA_ORA_GB");
  ora("CREATE TABLE QA_ORA_GB (ID NUMBER PRIMARY KEY);\nINSERT INTO QA_ORA_GB VALUES (1);\nCOMMIT;");
}

export default async function (page) {
  try {
    await oracleEditor(page);
    const sql = "BEGIN DELETE FROM QA_ORA_GB; END;";
    await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    await page.type(sql);
    await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
    await page.key("Escape", "Escape", 27);
    await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
    await page.key("Enter", "Enter", 13, CTRL);
    const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
    await page.waitFor(`document.querySelector('.modal.guard') !== null || (${cancel}?.disabled === true && ${status} !== '' && !${status}.includes('Executing'))`, 60_000);
    const guarded = await page.eval(`document.querySelector('.modal.guard') !== null`);
    if (guarded) await page.eval(`[...document.querySelectorAll('.modal.guard button')].find(b => b.textContent.trim() === 'Cancel').click()`);
    else await page.eval(`[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Rollback')?.click()`);
    assert.ok(guarded, `ran without confirmation: ${await page.eval(status)}`);
  } finally {
    dropAll("TABLE QA_ORA_GB");
  }
}
