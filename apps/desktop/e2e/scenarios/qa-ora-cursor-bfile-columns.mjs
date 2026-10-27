// Bug ORA-24 (low): Oracle values without a text form are lost or break the query. A cursor
// expression column (SELECT CURSOR(…)) is shown as NULL: the driver reads every value it cannot
// turn into text as NULL (crates/platypus-driver-oracle/src/convert.rs, Builder::Text), so a
// non-null value is shown as NULL. A BFILE column fails the whole query with ORA-00932
// "expression is of data type FILE, which is incompatible with expected data type BINARY"
// (Kind::Binary is asked of a BFILE locator). DBeaver shows the cursor's rows and the BFILE's
// directory/name.
// Steps: SELECT CURSOR(SELECT 1 FROM DUAL) c FROM DUAL; SELECT BFILENAME('DATA_PUMP_DIR',
// 'x.txt') f FROM DUAL.
// Expected: something other than NULL for the cursor (its rows or a placeholder), and the BFILE
// query to run (showing the locator's directory and file name). Actual: NULL, and ORA-00932.
import { assert, needsOracle, oracleEditor, oracleSetup, rows, runOra } from "../qa-ora.mjs";

export const skip = needsOracle();
export const setup = ({ dir }) => oracleSetup(dir);

export default async function (page) {
  await oracleEditor(page);
  const bfile = await runOra(page, "SELECT BFILENAME('DATA_PUMP_DIR', 'x.txt') f FROM DUAL");
  assert.equal(bfile.error, null, `BFILE: ${bfile.error}`);
  const cursor = await runOra(page, "SELECT CURSOR(SELECT 1 FROM DUAL) c FROM DUAL");
  assert.equal(cursor.error, null);
  assert.notDeepEqual(await rows(page), [[""]], "a cursor column is shown as NULL");
}
