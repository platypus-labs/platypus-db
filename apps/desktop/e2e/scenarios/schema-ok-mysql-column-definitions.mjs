// Regression guard (passes): the table editor on MySQL and MariaDB renames every column of a table with
// awkward definitions (expression defaults, a backslash and a quote in a default, BIT and SET defaults,
// ON UPDATE, a charset/collation, a comment with a quote, a STORED generated column) in one save; the
// definitions read back from information_schema are the same as before, only the names changed (the one the generated column reads keeps its name).
import assert from "node:assert/strict";
import { Q4, ENGINES, dropAll, lab, saveEngines, seed } from "../schema-helpers.mjs";
import { needsEngines } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("mysql", "mariadb");

const COLS = {
  mysql: `u varchar(36) DEFAULT (uuid()), d date DEFAULT (curdate()), j json DEFAULT (json_array()), bs varchar(10) DEFAULT 'a\\\\b', q varchar(10) DEFAULT 'it''s', bt bit(3) DEFAULT b'101', ts6 datetime(6) DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6), st set('a','b','c') DEFAULT 'a,b', cc varchar(10) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL DEFAULT 'zz' COMMENT 'it''s a comment', g int GENERATED ALWAYS AS (length(bs)) STORED NOT NULL`,
  mariadb: `u varchar(36) DEFAULT (uuid()), d date DEFAULT (curdate()), bs varchar(10) DEFAULT 'a\\\\b', q varchar(10) DEFAULT 'it''s', ts6 datetime(6) DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6), cc varchar(10) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL DEFAULT 'zz' COMMENT 'it''s a comment', g int AS (length(bs)) PERSISTENT`,
};

export function setup({ dir }) {
  for (const k of ["mysql", "mariadb"]) seed(dir, k, `CREATE TABLE S.t (id int AUTO_INCREMENT PRIMARY KEY, ${COLS[k]});`);
  saveEngines(dir, ["mysql", "mariadb"]);
}

const shape = (k) =>
  ENGINES[k]
    .root(`SELECT COLUMN_NAME, COLUMN_TYPE, IFNULL(COLUMN_DEFAULT, '<null>'), IS_NULLABLE, EXTRA, IFNULL(COLLATION_NAME, '-'), COLUMN_COMMENT FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = '${Q4}' AND TABLE_NAME = 't' ORDER BY ORDINAL_POSITION`)
    .trim()
    .split("\n")
    .map((l) => l.replace(/^[^\t]*\t/, ""));

export default async function (page, { dir }) {
  for (const k of ["mysql", "mariadb"]) {
    try {
      const before = shape(k);
      const l = await lab(page, dir, k);
      const r = await l.edit("t", (d) => {
        // `bs` stays: the generated column reads it.
        for (const c of d.columns) if (c.name !== "id" && c.name !== "bs") c.name = `${c.name}_r`;
      });
      assert.equal(r.error, null, `${k}: ${r.error}\n${r.sql}`);
      assert.deepEqual(shape(k), before, k);
    } finally {
      dropAll(k);
    }
  }
}

