// Bug ORA-01 (Oracle 23ai): a query that returns a JSON column fails as a whole with
// "unsupported Oracle type JSON", so no table with a JSON column can be opened or queried
// with SELECT *.
// Steps: CREATE TABLE QA_ORA_JSON (ID NUMBER PRIMARY KEY, J JSON), one row; in the editor run
// SELECT * FROM QA_ORA_JSON.
// Expected: 1 row, J shown as its JSON text (DBeaver shows it). Actual: the error
// "unsupported Oracle type JSON" and no rows.
import { assert, dropAll, needsOracle, ora, oracleEditor, oracleSetup, rows, runOra } from "../qa-ora.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_JSON");
  ora(`CREATE TABLE QA_ORA_JSON (ID NUMBER PRIMARY KEY, J JSON);
INSERT INTO QA_ORA_JSON VALUES (1, JSON('{"a": 1, "b": [true, null, "x"]}'));
COMMIT;`);
}

export default async function (page) {
  try {
    await oracleEditor(page);
    const { status, error } = await runOra(page, "SELECT * FROM QA_ORA_JSON");
    assert.equal(error, null, "querying a JSON column fails");
    assert.equal(status, "1 row");
    const [[id, json]] = await rows(page);
    assert.equal(id, "1");
    assert.deepEqual(JSON.parse(json), { a: 1, b: [true, null, "x"] });
  } finally {
    dropAll("TABLE QA_ORA_JSON");
  }
}
