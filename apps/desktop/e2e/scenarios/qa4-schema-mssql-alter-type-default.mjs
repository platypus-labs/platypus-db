// SCH4-13 (medium). On SQL Server the table editor cannot change the type of a column that has a default:
// the script is only `ALTER TABLE t ALTER COLUMN q bigint NOT NULL`, and SQL Server refuses it ("The
// object 'DF__t__q__...' is dependent on column 'q'") because the default constraint must be dropped
// first and added back after. (A default on a column renamed or whose default changes is handled by
// drop_mssql_default; a type change is not.)
// Steps: t (id int PRIMARY KEY, q int NOT NULL DEFAULT 5); table editor > q type bigint > Save.
// Expected: the script drops the default, alters the column and adds the default again; q is bigint
// with default 5. Actual: the save fails with the dependent-object error.
// Code: crates/platypus-schema/src/ddl.rs Ddl::alter_column, SQL Server branch (type_changed with an
// existing default).
import assert from "node:assert/strict";
import { Q4, ENGINES, dropAll, lab, saveEngines, seed } from "../qa4-schema.mjs";
import { needsEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("mssql");

export function setup({ dir }) {
  seed(dir, "mssql", `CREATE TABLE S.t (id int PRIMARY KEY, q int NOT NULL DEFAULT 5); INSERT INTO S.t (id) VALUES (1);`);
  saveEngines(dir, ["mssql"]);
}

export default async function (page, { dir }) {
  try {
    const l = await lab(page, dir, "mssql");
    const r = await l.edit("t", (d) => {
      d.columns[1].data_type = "bigint";
    });
    assert.equal(r.error, null, r.sql);
    const got = ENGINES.mssql
      .query(`SELECT TYPE_NAME(c.user_type_id) + ' ' + ISNULL(dc.definition, '-') FROM sys.columns c LEFT JOIN sys.default_constraints dc ON dc.parent_object_id = c.object_id AND dc.parent_column_id = c.column_id WHERE c.object_id = OBJECT_ID('${Q4}.t') AND c.name = 'q'`)
      .trim();
    assert.match(got, /^bigint \(\(?5\)?\)$/, got);
  } finally {
    dropAll("mssql");
  }
}
