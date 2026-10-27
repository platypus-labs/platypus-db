// Bug ORA-06: editing a DATE or TIMESTAMP cell on Oracle cannot be saved: the data editor
// writes the value as a plain string ('2021-02-03 04:05:06'), which Oracle converts with the
// session's NLS_DATE_FORMAT / NLS_TIMESTAMP_FORMAT (DD-MON-RR by default for the Instant
// Client) and refuses with ORA-01861 "literal does not match format string".
// (crates/platypus-sql/src/dml.rs, ValueKind::Timestamp falls back to quote_literal; the driver
// sets no NLS formats on connect.) The same literals reach Oracle from imports, copies to
// Oracle and the undo script.
// Steps: table QA_ORA_GD (ID NUMBER(5) PK, D DATE, T TIMESTAMP) with one row; Open data;
// set D to 2021-02-03 04:05:06 and T to 2021-02-03 04:05:06.5; Save.
// Expected: saved (e.g. TIMESTAMP '…' / TO_DATE(…, 'YYYY-MM-DD HH24:MI:SS') literals, or
// ISO NLS formats set on the session). Actual: ORA-01861.
import { assert, connectOracle, dropAll, editBar, focusCell, needsOracle, openData, ora, oracleSetup, save, setCell } from "../qa-ora.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_GD");
  ora(`CREATE TABLE QA_ORA_GD (ID NUMBER(5) PRIMARY KEY, D DATE, T TIMESTAMP);
INSERT INTO QA_ORA_GD VALUES (1, DATE '2020-01-15', TIMESTAMP '2020-01-15 10:00:00');
COMMIT;`);
}

export default async function (page) {
  try {
    await connectOracle(page);
    await openData(page, "QA_ORA_GD");
    await page.waitFor(`${editBar}.includes("to edit a cell")`, 15_000);
    await focusCell(page, 0, 1);
    await setCell(page, "2021-02-03 04:05:06");
    await focusCell(page, 0, 2);
    await setCell(page, "2021-02-03 04:05:06.5");
    await page.waitFor(`${editBar}.includes("1 row changed")`);
    const outcome = await save(page);
    assert.doesNotMatch(outcome, /ERROR/, outcome);
    assert.equal(
      ora("SELECT TO_CHAR(D, 'YYYY-MM-DD HH24:MI:SS') || '|' || TO_CHAR(T, 'YYYY-MM-DD HH24:MI:SS.FF1') FROM QA_ORA_GD;"),
      "2021-02-03 04:05:06|2021-02-03 04:05:06.5",
    );
  } finally {
    dropAll("TABLE QA_ORA_GD");
  }
}
