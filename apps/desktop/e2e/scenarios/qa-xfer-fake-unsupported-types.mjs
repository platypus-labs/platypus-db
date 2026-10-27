// XFER-06 (medium). Fake data suggests "Words" or "Whole number" for types they do not fit.
// Steps: a PostgreSQL table (id int PRIMARY KEY, tags text[], meta jsonb, ip inet, t time,
// iv interval); navigator › table › "Generate data…", keep the suggestions, 20 rows.
// Expected: generators that produce valid values (an array, a JSON value, an address, a time,
// an interval) — or "Default"/NULL for a type the app cannot fill — and 20 rows.
// Actual: tags, meta, ip and t get "Words", iv gets "Whole number", and the run fails with
// "malformed array literal: "do dolore …"" (then invalid json, inet, time, interval). The same
// happens to MySQL SET, YEAR and JSON columns and to ClickHouse Array(String) ("Array does
// not start with '['").
// Suspected: apps/desktop/src/transfer/generators.ts:24-57 (suggest() falls back to "text" for
// any unknown type and maps "interval" to integer through /int/).
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { db, dbQuiet, DBS, generate, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const TABLE = "qa_xfer_fk_types";

export const skip = () => skipUnless("pg");

export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  db("pg", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, tags text[], meta jsonb, ip inet, t time, iv interval)`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    const { kinds, out } = await generate(page, TABLE, [DBS.pg.name, "public"], 20);
    assert.match(out, /Added 20 rows/, `generators ${kinds}`);
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
