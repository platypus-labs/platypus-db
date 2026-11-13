// Bug ORA-07: Duplicate row (Ctrl+D) on an Oracle table with a virtual column cannot be
// saved: the INSERT names the virtual column ("INSERT operation disallowed on virtual
// columns", ORA-54013). The catalog query of the columns a table computes or numbers by
// itself has no Oracle branch (crates/platypus-sql/src/generated.rs, `query` returns None for
// Dialect::Oracle), so neither virtual columns (ALL_TAB_COLS.VIRTUAL_COLUMN) nor identities
// (ALL_TAB_IDENTITY_COLS, GENERATED ALWAYS refuses a value) are left to the database.
// The duplicated value of the computed column is also written as a string ('2').
// Steps: QA_ORA_GV (ID identity PK, A NUMBER(5), TWICE AS (A * 2) VIRTUAL) with one row;
// Open data; Ctrl+D on the row; Save.
// Expected: a second row with a new ID and TWICE computed. Actual: ORA-54013.
import { CTRL, assert, connectOracle, dropAll, editBar, focusCell, needsOracle, openData, ora, oracleSetup, save } from "../oracle-helpers.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_GV");
  ora(`CREATE TABLE QA_ORA_GV (ID NUMBER(5) GENERATED ALWAYS AS IDENTITY PRIMARY KEY, A NUMBER(5), TWICE NUMBER GENERATED ALWAYS AS (A * 2) VIRTUAL);
INSERT INTO QA_ORA_GV (A) VALUES (1);
COMMIT;`);
}

export default async function (page) {
  try {
    await connectOracle(page);
    await openData(page, "QA_ORA_GV");
    await page.waitFor(`${editBar}.includes("to edit a cell")`, 15_000);
    await focusCell(page, 0, 1);
    await page.key("d", "KeyD", 68, CTRL);
    await page.waitFor(`${editBar}.includes("1 row changed")`);
    const outcome = await save(page);
    assert.doesNotMatch(outcome, /ERROR/, outcome);
    assert.equal(ora("SELECT COUNT(*) || '|' || COUNT(DISTINCT ID) || '|' || SUM(TWICE) FROM QA_ORA_GV;"), "2|2|4");
  } finally {
    dropAll("TABLE QA_ORA_GV");
  }
}
