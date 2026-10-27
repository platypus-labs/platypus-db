// XFER-09 (medium). An empty table with a foreign key to itself cannot get fake rows.
// Steps: a PostgreSQL table tree (id int PRIMARY KEY, parent_id int REFERENCES tree (id),
// name text), empty; navigator › table › "Generate data…", keep the suggestions, 5 rows.
// Expected: 5 rows — parent_id is nullable, so the first rows can be roots (NULL) and later
// ones point at them; or NULL suggested while the table is empty.
// Actual: "Nothing was added: id refers to tree, which has no rows to point at: fill it
// first" — impossible for a self reference, and the message names the referenced column
// (id), not the foreign key column (parent_id). Workaround: pick NULL for parent_id.
// Suspected: crates/platypus-transfer/src/fake.rs:452-479 (keys() errors on an empty table,
// naming `columns`, the referenced ones) and 619-640.
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { db, dbQuiet, DBS, generate, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const TABLE = "qa_xfer_fk_tree";

export const skip = () => skipUnless("pg");

export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  db("pg", `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, parent_id int REFERENCES ${TABLE} (id), name text)`);
}

export default async function (page) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    const { kinds, out } = await generate(page, TABLE, [DBS.pg.name, "public"], 5);
    assert.match(out, /Added 5 rows/, `generators ${kinds}`);
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}`);
  }
}
