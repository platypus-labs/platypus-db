// Regression guard (passes): Compare with… on MariaDB. Two identical schemas need nothing ("nothing
// to change"); a schema with a self-reference, a composite key and its foreign key, two tables
// referencing each other, defaults, NOT NULL, a unique column, a check, odd names (spaces, a
// reserved word, an accent) and a view, compared with a changed copy (types, NULL, defaults,
// an index, a column more, a table missing, two tables more): the script opened on the target
// runs and a second comparison finds nothing to change.
import assert from "node:assert/strict";
import { ENGINES, SRC, DST, needsEngines, okRoundTrip, saveEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("mariadb");

export function setup({ dir }) {
  ENGINES.mariadb.seed(dir, SRC, "");
  ENGINES.mariadb.seed(dir, DST, "");
  saveEngines(dir, ["mariadb"]);
}

export default async function (page, { dir }) {
  try {
    const r = await okRoundTrip(page, dir, "mariadb");
    assert.ok(r.sql.includes("CREATE TABLE"), r.sql);
  } finally {
    ENGINES.mariadb.drop?.(SRC);
    ENGINES.mariadb.drop?.(DST);
  }
}
