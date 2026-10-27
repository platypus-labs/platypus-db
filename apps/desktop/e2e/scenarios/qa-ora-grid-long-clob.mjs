// Bug ORA-08: a CLOB cell edited to more than 4,000 bytes cannot be saved on Oracle: the
// value goes into the UPDATE as one string literal, and Oracle takes at most 4,000 bytes in a
// literal ("string literal too long", ORA-01704). BLOBs over 2,000 bytes fail the same way
// (HEXTORAW('…') of a literal), and so does anything that writes rows with literals (import,
// copy to Oracle, the undo script) as soon as a text is long. Long values need binds, or
// TO_CLOB('…') || TO_CLOB('…') chunks of under 4,000 bytes.
// Steps: QA_ORA_GC (ID NUMBER(5) PK, C CLOB) with (1, 'short'); Open data; set C to 5,000
// characters; Save.
// Expected: saved, LENGTH(C) = 5000. Actual: ORA-01704 string literal too long.
import { assert, connectOracle, dropAll, editBar, focusCell, needsOracle, openData, ora, oracleSetup, save, setCell } from "../qa-ora.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_GC");
  ora(`CREATE TABLE QA_ORA_GC (ID NUMBER(5) PRIMARY KEY, C CLOB);
INSERT INTO QA_ORA_GC VALUES (1, 'short');
COMMIT;`);
}

export default async function (page) {
  try {
    await connectOracle(page);
    await openData(page, "QA_ORA_GC");
    await page.waitFor(`${editBar}.includes("to edit a cell")`, 15_000);
    await focusCell(page, 0, 1);
    await setCell(page, "x".repeat(4999) + "!");
    await page.waitFor(`${editBar}.includes("1 row changed")`);
    const outcome = await save(page);
    assert.doesNotMatch(outcome, /ERROR/, outcome);
    assert.equal(ora("SELECT LENGTH(C) || '|' || SUBSTR(C, -1) FROM QA_ORA_GC;"), "5000|!");
  } finally {
    dropAll("TABLE QA_ORA_GC");
  }
}
