// SCH4-07 (high). On SQL Server the table editor's type / NULL change is `ALTER COLUMN c type NULL`
// without COLLATE, which resets the column to the database's default collation: a case-sensitive
// column silently becomes case-insensitive (comparisons, GROUP BY, UNIQUE indexes change meaning).
// Steps: t (id int PRIMARY KEY, d varchar(10) COLLATE Latin1_General_CS_AS NULL, n nvarchar(10)
// COLLATE Latin1_General_100_CI_AS_SC NOT NULL); table editor > d type varchar(20), n nullable > Save.
// Expected: both columns keep their collation. Actual: both are SQL_Latin1_General_CP1_CI_AS.
// Code: crates/platypus-schema/src/lib.rs load() reads no collation for SQL Server (the MySQL and
// SQLite branches do), and ddl.rs alter_column writes `ALTER COLUMN {c} {ty} {null}` for SQL Server.
import assert from "node:assert/strict";
import { Q4, ENGINES, lab, saveEngines, seed } from "../qa4-schema.mjs";
import { needsEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("mssql");

export function setup({ dir }) {
  seed(dir, "mssql", `CREATE TABLE S.t (id int PRIMARY KEY, d varchar(10) COLLATE Latin1_General_CS_AS NULL, n nvarchar(10) COLLATE Latin1_General_100_CI_AS_SC NOT NULL DEFAULT 'x');`);
  saveEngines(dir, ["mssql"]);
}

export default async function (page, { dir }) {
  try {
    const l = await lab(page, dir, "mssql");
    const r = await l.edit("t", (d) => {
      d.columns[1].data_type = "varchar(20)";
      d.columns[2].nullable = true;
    });
    assert.equal(r.error, null, r.sql);
    const got = ENGINES.mssql
      .query(`SELECT STRING_AGG(name + '=' + collation_name, ',') WITHIN GROUP (ORDER BY column_id) FROM sys.columns WHERE object_id = OBJECT_ID('${Q4}.t') AND collation_name IS NOT NULL`)
      .trim();
    assert.equal(got, "d=Latin1_General_CS_AS,n=Latin1_General_100_CI_AS_SC", `${got}\n${r.sql}`);
  } finally {
    ENGINES.mssql.drop(Q4);
  }
}
