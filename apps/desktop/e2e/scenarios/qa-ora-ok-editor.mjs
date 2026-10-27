// Regression guard (passes): the SQL editor on Oracle. A script mixes plain SQL with "/" lines,
// an anonymous block, CREATE TRIGGER using :NEW / :OLD (not asked as parameters) and q'[…]'
// quotes; an error points at its line; :name parameters are asked and substituted; a long
// query is cancelled and the session still answers; manual commit keeps a row from
// other sessions until Commit, Rollback drops one, and a savepoint rolls part back.
import { CTRL, assert, button, clearStatus, dropAll, idle, needsOracle, ora, oracleEditor, oracleSetup, rows, runOra, visibleTab } from "../qa-ora.mjs";
import { answerParams, setEditor } from "../qa-sql.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_ED");
}

const SCRIPT = `CREATE TABLE QA_ORA_ED (ID NUMBER PRIMARY KEY, NAME VARCHAR2(20))
/
CREATE OR REPLACE TRIGGER QA_ORA_ED_TRG BEFORE INSERT OR UPDATE ON QA_ORA_ED FOR EACH ROW
BEGIN
  :NEW.NAME := UPPER(:NEW.NAME);
  IF :OLD.ID IS NOT NULL AND :NEW.ID <> :OLD.ID THEN
    RAISE_APPLICATION_ERROR(-20001, 'no key change');
  END IF;
END;
/
BEGIN
  INSERT INTO QA_ORA_ED VALUES (1, q'[it's]');
  INSERT INTO QA_ORA_ED VALUES (2, 'b;c');
END;
/
SELECT ID, NAME FROM QA_ORA_ED ORDER BY ID;`;

export default async function (page) {
  try {
    await oracleEditor(page);
    const script = await runOra(page, SCRIPT, true);
    assert.equal(script.error, null);
    assert.deepEqual(await rows(page), [["1", "IT'S"], ["2", "B;C"]]);
    assert.equal(ora("SELECT STATUS FROM USER_OBJECTS WHERE OBJECT_NAME = 'QA_ORA_ED_TRG';"), "VALID");

    // The error marks its line.
    const bad = await runOra(page, "SELECT ID,\n  NO_COL\nFROM QA_ORA_ED");
    assert.match(bad.error, /ORA-00904/);
    await page.waitFor(`[...${visibleTab}.querySelectorAll('.cm-gutterElement')].some(g => g.textContent === '2' && g.closest('.cm-gutter') && g.parentElement.parentElement.querySelector('.cm-lintPoint-error, .cm-lint-marker-error') !== null) || ${visibleTab}.querySelector('.cm-lintRange-error, .cm-lint-marker-error') !== null`, 5000);

    // Parameters.
    await setEditor(page, "SELECT NAME FROM QA_ORA_ED WHERE ID = :id AND NAME <> :skip");
    await clearStatus(page);
    await page.key("Enter", "Enter", 13, CTRL);
    await answerParams(page, ["2", "x"]);
    await idle(page, "s.includes('row')");
    assert.deepEqual(await rows(page), [["B;C"]]);

    // Cancel a long call.
    await setEditor(page, "SELECT COUNT(*) FROM all_objects a, all_objects b, all_objects c");
    await clearStatus(page);
    await page.key("Enter", "Enter", 13, CTRL);
    await page.waitFor(`${button(visibleTab, "Cancel")}?.disabled === false`, 10_000);
    await page.sleep(1000);
    const started = Date.now();
    await page.eval(`${button(visibleTab, "Cancel")}.click()`);
    await idle(page, "true", 15_000);
    assert.ok(Date.now() - started < 10_000, "cancel took too long");
    assert.match(await page.eval(`${visibleTab}.textContent`), /cancel/i);
    assert.equal((await runOra(page, "SELECT 'alive' FROM DUAL")).error, null);

    // Manual commit.
    await page.eval(`${button(visibleTab, "Auto-commit")}.click()`);
    await page.waitFor(`${button(visibleTab, "Manual commit")} !== undefined`);
    await runOra(page, "INSERT INTO QA_ORA_ED VALUES (3, 'c')");
    assert.equal(ora("SELECT COUNT(*) FROM QA_ORA_ED;"), "2", "uncommitted row seen by another session");
    await page.eval(`${button(visibleTab, "Commit")}.click()`);
    await page.waitFor(`${visibleTab}.querySelector('.tx-open') === null`);
    assert.equal(ora("SELECT COUNT(*) FROM QA_ORA_ED;"), "3");
    await runOra(page, "INSERT INTO QA_ORA_ED VALUES (4, 'd')");
    await runOra(page, "SAVEPOINT qa_sp");
    await runOra(page, "INSERT INTO QA_ORA_ED VALUES (5, 'e')");
    await runOra(page, "ROLLBACK TO SAVEPOINT qa_sp");
    await page.eval(`${button(visibleTab, "Commit")}.click()`);
    await page.waitFor(`${visibleTab}.querySelector('.tx-open') === null`);
    assert.equal(ora("SELECT LISTAGG(ID, ',') WITHIN GROUP (ORDER BY ID) FROM QA_ORA_ED;"), "1,2,3,4");
    await runOra(page, "DELETE FROM QA_ORA_ED WHERE ID > 0");
    await page.eval(`${button(visibleTab, "Rollback")}.click()`);
    await page.waitFor(`${visibleTab}.querySelector('.tx-open') === null`);
    assert.equal(ora("SELECT COUNT(*) FROM QA_ORA_ED;"), "4");
  } finally {
    dropAll("TABLE QA_ORA_ED");
  }
}
