// Bug ORA-02 (Oracle 23ai): a query that returns a VECTOR column fails as a whole with
// "unknown Oracle type number 2033".
// Steps: CREATE TABLE QA_ORA_VEC (ID NUMBER PRIMARY KEY, V VECTOR(3, FLOAT32)), one row
// TO_VECTOR('[1.5, 2, -3]'); in the editor run SELECT * FROM QA_ORA_VEC.
// Expected: 1 row, V shown as its text form ([1.5, 2, -3] or Oracle's
// [1.5E+000,2.0E+000,-3.0E+000]). Actual: the error "unknown Oracle type number 2033".
import { assert, dropAll, needsOracle, ora, oracleEditor, oracleSetup, rows, runOra } from "../qa-ora.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_VEC");
  ora(`CREATE TABLE QA_ORA_VEC (ID NUMBER PRIMARY KEY, V VECTOR(3, FLOAT32));
INSERT INTO QA_ORA_VEC VALUES (1, TO_VECTOR('[1.5, 2, -3]'));
COMMIT;`);
}

export default async function (page) {
  try {
    await oracleEditor(page);
    const { status, error } = await runOra(page, "SELECT * FROM QA_ORA_VEC");
    assert.equal(error, null, "querying a VECTOR column fails");
    assert.equal(status, "1 row");
    const [[, vector]] = await rows(page);
    const numbers = vector.replace(/[[\]\s]/g, "").split(",").map(Number);
    assert.deepEqual(numbers, [1.5, 2, -3], vector);
  } finally {
    dropAll("TABLE QA_ORA_VEC");
  }
}
