// SCH4-14 (medium). In the MySQL table editor, widening the primary key of a table whose key column is
// AUTO_INCREMENT (a common step: make it a composite key) always fails. The script drops the key in one
// statement and adds the new one in the next; MySQL and MariaDB refuse the first ("there can be only one
// auto column and it must be defined as a key") and the table is left as it was.
// Steps: t (id int NOT NULL AUTO_INCREMENT PRIMARY KEY, a int NOT NULL DEFAULT 0); table editor > tick
// primary key on a too > Save.
// Expected: one statement, ALTER TABLE t DROP PRIMARY KEY, ADD PRIMARY KEY (id, a), and PRIMARY KEY (`id`,`a`)
// afterwards. Actual: ERROR 1075 on `DROP PRIMARY KEY`.
// Code: crates/platypus-schema/src/ddl.rs Ddl::alter (MySql: "DROP PRIMARY KEY" pushed alone, the new key
// added later with "ADD PRIMARY KEY").
import assert from "node:assert/strict";
import { Q4, ENGINES, dropAll, lab, saveEngines, seed } from "../qa4-schema.mjs";
import { needsEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("mysql");

export function setup({ dir }) {
  seed(dir, "mysql", `CREATE TABLE S.t (id int NOT NULL AUTO_INCREMENT PRIMARY KEY, a int NOT NULL DEFAULT 0);`);
  saveEngines(dir, ["mysql"]);
}

export default async function (page, { dir }) {
  try {
    const l = await lab(page, dir, "mysql");
    const r = await l.edit("t", (d) => {
      d.columns[1].primary_key = true;
    });
    assert.equal(r.error, null, r.sql);
    assert.equal(
      ENGINES.mysql.root(`SELECT GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX) FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = '${Q4}' AND TABLE_NAME = 't' AND INDEX_NAME = 'PRIMARY'`).trim(),
      "id,a",
    );
  } finally {
    dropAll("mysql");
  }
}
