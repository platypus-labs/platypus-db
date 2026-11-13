// Bug ORA-19: "Generate data…" on Oracle ignores the length of VARCHAR2 / NVARCHAR2 columns,
// so nothing is generated: "value too large for column … (actual: 40, maximum: 20)"
// (ORA-12899). The length check that cuts generated text knows varchar, nvarchar and char but
// not Oracle's VARCHAR2 / NVARCHAR2 nor the "20 CHAR" / "20 BYTE" units the navigator writes
// (crates/platypus-transfer/src/import.rs, text_length, used by fake.rs). A VARCHAR2 length
// counts bytes by default, so the cut also has to count bytes there.
// Steps: QA_ORA_FK (ID NUMBER GENERATED ALWAYS AS IDENTITY PK, V VARCHAR2(20), W VARCHAR2(8 CHAR));
// navigator › Generate data…, keep the suggested generators, 10 rows.
// Expected: 10 rows. Actual: Nothing was added: ORA-12899 (with only V), and with the
// VARCHAR2(8 CHAR) column even an ORA-00600 internal error "[qctchr : bfl]" from the server for
// the generated INSERT.
import { assert, CONN, dropAll, needsOracle, ora, oracleSetup } from "../oracle-helpers.mjs";
import { generate, showTable } from "../transfer-helpers.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  dropAll("TABLE QA_ORA_FK");
  ora("CREATE TABLE QA_ORA_FK (ID NUMBER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, V VARCHAR2(20), W VARCHAR2(8 CHAR));");
}

export default async function (page) {
  try {
    await showTable(page, CONN, "PLATYPUS", "QA_ORA_FK");
    const { out } = await generate(page, "QA_ORA_FK", [CONN, "PLATYPUS"], 10);
    assert.match(out, /Added 10 rows/, out);
    assert.equal(ora("SELECT COUNT(*) FROM QA_ORA_FK;"), "10");
  } finally {
    dropAll("TABLE QA_ORA_FK");
  }
}
