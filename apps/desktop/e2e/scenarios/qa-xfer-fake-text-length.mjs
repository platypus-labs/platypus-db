// XFER-04 (high). Fake data ignores the length of text columns: nothing is generated.
// Steps: a PostgreSQL table (id serial PRIMARY KEY, code varchar(5), cc char(2), name
// varchar(8)); navigator › table › "Generate data…", keep the suggested generators, 20 rows.
// Expected: 20 rows, the text cut to each column's length (or generators that fit: a 2-letter
// code for char(2)).
// Actual: "Nothing was added: value too long for type character varying(5)": Words, Full name
// and the other text generators write values longer than the column, and no generator offered
// fits varchar(5) except NULL. MySQL ("Data too long") and SQL Server ("String or binary data
// would be truncated") fail the same way.
// Suspected: crates/platypus-transfer/src/fake.rs:311-370 (text() has no maximum length) and
// apps/desktop/src/transfer/GenerateDataDialog.tsx:94-101 (the column's length is not passed).
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { db, dbQuiet, DBS, generate, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const TABLE = "qa_xfer_fk_len";

export const skip = () => skipUnless("pg");

export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  db("pg", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id serial PRIMARY KEY, code varchar(5), cc char(2), name varchar(8))`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    const { out } = await generate(page, TABLE, [DBS.pg.name, "public"], 20);
    assert.match(out, /Added 20 rows/);
    assert.equal(db("pg", `SELECT count(*) FROM ${TABLE}`), "20");
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
