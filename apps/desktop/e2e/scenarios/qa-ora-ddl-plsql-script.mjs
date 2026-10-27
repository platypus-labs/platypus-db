// Bug ORA-11: the DDL the navigator shows for an Oracle trigger (and for a type with a body)
// cannot be run back: DBMS_METADATA returns the CREATE TRIGGER block followed directly by
// "ALTER TRIGGER … ENABLE" (the type: CREATE TYPE then CREATE TYPE BODY) with no "/" line
// between them, so the editor's script runner, which ends PL/SQL at a "/" line, sends both
// as one statement and Oracle compiles the ALTER into the trigger body. The DDL query does
// not ask DBMS_METADATA for SQL terminators (crates/platypus-driver-oracle/src/meta.rs,
// ddl_sql; DBMS_METADATA.SET_TRANSFORM_PARAM(…, 'SQLTERMINATOR', TRUE) adds them).
// Steps: a trigger QA_ORA_DT_TRG; Open details › Edit DDL in editor; run it as a script
// (Alt+X); then check the trigger's status.
// Expected: the script runs and the trigger stays VALID. Actual: the trigger is recreated
// INVALID (the ALTER became part of its body) and the run reports no error.
import { ALT, assert, clearStatus, connectOracle, dropAll, idle, needsOracle, ora, oracleSetup, visibleTab } from "../qa-ora.mjs";
import { menu, row } from "../qa-nav-helpers.mjs";

export const skip = needsOracle();
const objTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_DT");
  ora(`CREATE TABLE QA_ORA_DT (ID NUMBER PRIMARY KEY, N NUMBER);
CREATE OR REPLACE TRIGGER QA_ORA_DT_TRG BEFORE INSERT ON QA_ORA_DT FOR EACH ROW
BEGIN
  :NEW.N := NVL(:NEW.N, 0);
END;
/`);
}

export default async function (page) {
  try {
    await connectOracle(page, "Triggers");
    await page.eval(`${row("Triggers")}.getAttribute('aria-expanded') === 'true' || ${row("Triggers")}.querySelector('.twisty').click()`);
    await page.waitFor(`${row("QA_ORA_DT_TRG")} !== undefined`, 20_000);
    await menu(page, "QA_ORA_DT_TRG", "Open details");
    await page.waitFor(`[...(${objTab}?.querySelectorAll('button') ?? [])].some(b => b.textContent.trim() === 'Edit DDL in editor')`, 20_000);
    await page.eval(`[...${objTab}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Edit DDL in editor').click()`);
    await page.waitFor(`(${visibleTab}?.querySelector('.cm-content')?.textContent ?? '').includes('ALTER TRIGGER')`, 15_000);
    await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
    await clearStatus(page);
    await page.key("x", "KeyX", 88, ALT);
    await idle(page);
    assert.equal(ora("SELECT STATUS FROM USER_OBJECTS WHERE OBJECT_NAME = 'QA_ORA_DT_TRG';"), "VALID");
  } finally {
    dropAll("TABLE QA_ORA_DT");
  }
}
