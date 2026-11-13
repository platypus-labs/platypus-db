// Bug ORA-17: "Copy to a database" into Oracle fails for ordinary tables. The rows go in as
// INSERTs with plain string literals, so (1) a date or timestamp column fails with ORA-01843
// "An invalid month was specified" (the text is read with the session's NLS formats, DD-MON-RR
// by default) and (2) a text over 4,000 bytes fails with ORA-01704 "string literal too long".
// Either error stops the copy: "Nothing was imported". Same roots as oracle-grid-date-literals
// and oracle-grid-long-clob, on the transfer path (crates/platypus-transfer, literals from
// platypus-sql dml).
// Steps: PostgreSQL table qa_ora_cp (id int PK, d date, ts timestamp, note text) with one row
// (note 5,000 characters); navigator › qa_ora_cp › Copy to a database… › QA Oracle; Copy.
// Expected: 1 row in Oracle with the same date, timestamp and text. Actual: ORA-01843 (and,
// with no date columns, ORA-01704).
import { assert, CONN, dropAll, installClient, needsOracle, ora, oracleToml } from "../oracle-helpers.mjs";
import { closeModal, copyTarget, db, dbQuiet, DBS, importDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../transfer-helpers.mjs";

const T = "qa_ora_cp";

export const skip = async () => (await needsOracle()()) ?? (await skipUnless("pg"));

export function setup({ dir }) {
  installClient(dir);
  saveConnections(dir, ["pg"], `\n${oracleToml("00000000-0000-4000-8000-0000000a0001", CONN)}`);
  db("pg", `DROP TABLE IF EXISTS ${T}; CREATE TABLE ${T} (id int PRIMARY KEY, d date, ts timestamp(3), note text);
     INSERT INTO ${T} VALUES (1, '2024-02-29', '2024-01-01 12:34:56.789', repeat('x', 4999) || '!');`);
  dropAll(`TABLE "${T}"`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.pg.name, "public", T);
    await menu(page, T, "Copy to a database", importDialog, [DBS.pg.name, "public"]);
    assert.equal(await copyTarget(page, CONN), null);
    const outcome = await runJob(page, importDialog, "Copy", 120_000);
    await closeModal(page, importDialog);
    assert.match(outcome, /Imported 1 row/, outcome);
    assert.equal(
      ora(`SELECT TO_CHAR("d", 'YYYY-MM-DD') || '|' || TO_CHAR("ts", 'YYYY-MM-DD HH24:MI:SS.FF3') || '|' || LENGTH("note") FROM "${T}";`),
      "2024-02-29|2024-01-01 12:34:56.789|5000",
    );
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${T}`);
    dropAll(`TABLE "${T}"`);
  }
}
