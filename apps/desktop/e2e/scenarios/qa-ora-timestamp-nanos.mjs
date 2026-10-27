// Bug ORA-03: TIMESTAMP(9) values lose their last three digits: the driver reads every
// timestamp as microseconds (crates/platypus-driver-oracle/src/convert.rs, Kind::Timestamp
// -> TimestampMicrosecond), so .123456789 is shown, copied and exported as .123456. A grid
// edit of another column keeps the row, but copying or exporting the value writes the wrong
// time back elsewhere.
// Steps: CREATE TABLE QA_ORA_TS9 (ID NUMBER PRIMARY KEY, T TIMESTAMP(9)) with
// TIMESTAMP '2024-02-29 13:45:59.123456789'; SELECT * in the editor; copy the cell.
// Expected: 2024-02-29 13:45:59.123456789. Actual: 2024-02-29 13:45:59.123456.
import { assert, dropAll, needsOracle, ora, oracleEditor, oracleSetup, rows, runOra } from "../qa-ora.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_TS9");
  ora(`CREATE TABLE QA_ORA_TS9 (ID NUMBER PRIMARY KEY, T TIMESTAMP(9));
INSERT INTO QA_ORA_TS9 VALUES (1, TIMESTAMP '2024-02-29 13:45:59.123456789');
COMMIT;`);
}

export default async function (page) {
  try {
    await oracleEditor(page);
    const { error } = await runOra(page, "SELECT T FROM QA_ORA_TS9");
    assert.equal(error, null);
    const [[value]] = await rows(page);
    assert.equal(value, "2024-02-29 13:45:59.123456789");
  } finally {
    dropAll("TABLE QA_ORA_TS9");
  }
}
