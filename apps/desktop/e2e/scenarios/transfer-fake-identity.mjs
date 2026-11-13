// XFER-05 (medium). Fake data fills identity columns, which the database refuses.
// Steps: a PostgreSQL table (id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, name text);
// navigator › table › "Generate data…", keep the suggestions, 20 rows.
// Expected: id left to the database ("Default (not filled)" suggested), 20 rows added.
// Actual: id is suggested "Next key (after the largest)" and the run fails: "cannot insert a
// non-DEFAULT value into column "id"". A SQL Server IDENTITY column fails the same way
// ("Cannot insert explicit value for identity column … IDENTITY_INSERT is set to OFF").
// Workaround: switch id to "Default (not filled)".
// Suspected: apps/desktop/src/transfer/generators.ts:31-39 (an identity column has no
// `default`, so the primary key rule picks next_key; ColumnMeta's identity is not looked at).
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { db, dbQuiet, DBS, generate, saveConnections, showTable, skipUnless } from "../transfer-helpers.mjs";

const TABLE = "qa_xfer_fk_ident";

export const skip = () => skipUnless("pg");

export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  db("pg", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, name text)`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    const { kinds, out } = await generate(page, TABLE, [DBS.pg.name, "public"], 20);
    assert.match(out, /Added 20 rows/, `generators ${kinds}`);
    assert.equal(db("pg", `SELECT count(*) FROM ${TABLE}`), "20");
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
