// XFER-07 (medium). Fake whole numbers run from 1 to 1000 whatever the column holds.
// Steps: a MySQL table (id int PRIMARY KEY, tiny tinyint, flag tinyint(1)); navigator ›
// table › "Generate data…", keep the suggestions, 20 rows.
// Expected: numbers within the column's range (TINYINT is -128..127) and true/false for
// tinyint(1), MySQL's BOOLEAN; or bounds to type in the dialog.
// Actual: both get "Whole number" 1..1000 and the run fails: "Out of range value for column
// 'tiny'". The dialog has no field for the bounds (Generator has min/max, the UI never shows
// them), so a CHECK (n BETWEEN 1 AND 10) on PostgreSQL cannot be satisfied either.
// Suspected: apps/desktop/src/transfer/GenerateDataDialog.tsx:46-51 (fixed 1..1000, 0..1000)
// and apps/desktop/src/transfer/generators.ts:28,51 (tinyint(1) is not a boolean).
// Needs MySQL from docker/compose.yml.
import assert from "node:assert/strict";
import { db, dbQuiet, DBS, generate, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const TABLE = "qa_xfer_fk_range";

export const skip = () => skipUnless("mysql");

export function setup({ dir }) {
  saveConnections(dir, ["mysql"]);
  db("mysql", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, tiny tinyint, flag tinyint(1))`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.mysql.name, "platypus", TABLE);
    const { kinds, out } = await generate(page, TABLE, [DBS.mysql.name, "platypus"], 20);
    assert.match(out, /Added 20 rows/, `generators ${kinds}`);
    assert.equal(db("mysql", `SELECT count(*) FROM ${TABLE} WHERE flag IN (0, 1)`), "20");
  } finally {
    dbQuiet("mysql", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
