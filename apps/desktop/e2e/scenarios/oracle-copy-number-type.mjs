// Bug ORA-18: copying an Oracle table to another database turns NUMBER columns without a
// declared precision (plain NUMBER, INTEGER = NUMBER(38), virtual columns computed as NUMBER)
// into TEXT columns: the proposed target type comes from the result's Arrow type, which is
// Utf8 for these (crates/platypus-driver-oracle/src/convert.rs, `kind`), not from the Oracle
// type kept in the field metadata. The copied numbers then sort and compare as strings.
// Same root as oracle-number-as-text.
// Steps: Oracle QA_ORA_CN (ID NUMBER(10) PK, QTY NUMBER, BIG INTEGER) with one row;
// navigator › QA_ORA_CN › Copy to a database… › QA PG; Copy.
// Expected: QTY and BIG numeric in PostgreSQL (numeric). Actual: text.
import { assert, CONN, dropAll, installClient, needsOracle, ora, oracleToml } from "../oracle-helpers.mjs";
import { closeModal, copyTarget, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../transfer-helpers.mjs";

export const skip = async () => (await needsOracle()()) ?? (await skipUnless("pg"));

export function setup({ dir }) {
  installClient(dir);
  saveConnections(dir, ["pg"], `\n${oracleToml("00000000-0000-4000-8000-0000000a0001", CONN)}`);
  dropAll("TABLE QA_ORA_CN");
  ora(`CREATE TABLE QA_ORA_CN (ID NUMBER(10) PRIMARY KEY, QTY NUMBER, BIG INTEGER);
INSERT INTO QA_ORA_CN VALUES (1, 12.5, 12345678901234567890);
COMMIT;`);
  db("pg", `DROP TABLE IF EXISTS "QA_ORA_CN"`);
}

export default async function (page) {
  try {
    await showTable(page, CONN, "PLATYPUS", "QA_ORA_CN");
    await menu(page, "QA_ORA_CN", "Copy to a database", importDialog, [CONN, "PLATYPUS"]);
    assert.equal(await copyTarget(page, DBS.pg.name), null);
    assert.match(await runJob(page, importDialog, "Copy", 60_000), /Imported 1 row/);
    await closeModal(page, importDialog);
    const types = db("pg", `SELECT string_agg(column_name || ' ' || data_type, ', ' ORDER BY ordinal_position) FROM information_schema.columns WHERE table_name = 'QA_ORA_CN'`);
    assert.doesNotMatch(types, /QTY text|BIG text/, types);
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS "QA_ORA_CN"`);
    dropAll("TABLE QA_ORA_CN");
  }
}
