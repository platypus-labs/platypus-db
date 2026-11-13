// Bug ORA-21: a result exported from Oracle as SQL INSERTs cannot be run back on Oracle. Dates
// and timestamps are written as ISO strings ('2024-02-29T00:00:00'), which Oracle reads with
// the session's NLS formats and refuses (ORA-01861); NUMBER columns without precision are
// quoted as strings ('10', see oracle-number-as-text), and JSON export writes them as strings
// too ("QTY":"10"). The SQL exporter should write DATE/TIMESTAMP literals (or TO_DATE /
// TO_TIMESTAMP with an explicit format) for Oracle.
// Steps: SELECT ID, QTY, DATE '2024-02-29' D, TIMESTAMP '2024-01-02 03:04:05.5' T FROM a table;
// Export… › SQL INSERT, table QA_ORA_EX2; run the file in the editor against a table of the
// same shape.
// Expected: the rows inserted. Actual: ORA-01861 literal does not match format string.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { assert, dropAll, needsOracle, ora, oracleEditor, oracleSetup, runOra } from "../oracle-helpers.mjs";
import { exportResult } from "../transfer-helpers.mjs";

export const skip = needsOracle();
const SELECT = "SELECT ID, QTY, DATE '2024-02-29' D, TIMESTAMP '2024-01-02 03:04:05.5' T FROM QA_ORA_EX";

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_EX", "TABLE QA_ORA_EX2");
  ora(`CREATE TABLE QA_ORA_EX (ID NUMBER(10) PRIMARY KEY, QTY NUMBER);
INSERT INTO QA_ORA_EX VALUES (1, 10);
COMMIT;
CREATE TABLE QA_ORA_EX2 AS ${SELECT} WHERE 1 = 0;`);
}

export default async function (page, { dir }) {
  try {
    await oracleEditor(page);
    assert.equal((await runOra(page, SELECT)).status, "1 row");
    const path = join(dir, "out.sql");
    assert.match(await exportResult(page, path, { format: "sql", table: "QA_ORA_EX2" }), /Exported 1 row/);
    const replay = await runOra(page, readFileSync(path, "utf8"), true);
    assert.equal(replay.error, null, readFileSync(path, "utf8"));
    assert.equal(ora("SELECT ID || '|' || QTY || '|' || TO_CHAR(D, 'YYYY-MM-DD') || '|' || TO_CHAR(T, 'HH24:MI:SS.FF1') FROM QA_ORA_EX2;"), "1|10|2024-02-29|03:04:05.5");
  } finally {
    dropAll("TABLE QA_ORA_EX", "TABLE QA_ORA_EX2");
  }
}
