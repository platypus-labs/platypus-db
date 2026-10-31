// QA5-NAV-8 (medium). "Generate DDL" of a SQL Server table drops the COLLATE of a column that
// has its own: `name nvarchar(50) COLLATE Latin1_General_CS_AS NOT NULL` is printed as
// `name nvarchar(50) NOT NULL`. A script run on another server gets the database's collation:
// a case-sensitive column becomes case-insensitive, and a unique key on it starts to refuse
// 'A' next to 'a'. (Also left out: SPARSE, the name of a default constraint, MS_Description.)
// Steps: CREATE TABLE t (name nvarchar(50) COLLATE Latin1_General_CS_AS NOT NULL);
//   right-click t > Generate DDL.
// Expected: the column keeps `COLLATE Latin1_General_CS_AS`. Actual: no COLLATE.
// Code: crates/platypus-driver-mssql/src/ddl.rs (the columns query reads no collation_name).
import assert from "node:assert/strict";
import { DBS, connectTo, editorText, menu, needs, row, saveConnections } from "../qa-nav-helpers.mjs";

export const skip = needs("sqlserver");

const DROP = "IF OBJECT_ID('dbo.qa5_coll') IS NOT NULL DROP TABLE dbo.qa5_coll;";

export function setup({ dir }) {
  DBS.sqlserver.exec(`${DROP} CREATE TABLE dbo.qa5_coll (id INT PRIMARY KEY, name NVARCHAR(50) COLLATE Latin1_General_CS_AS NOT NULL);`);
  saveConnections(dir, DBS.sqlserver.connection("QA MS", "00000000-0000-4000-8000-0000000a5d02"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA MS", "Platypus-2026");
    await page.waitFor(`${row("qa5_coll")} !== undefined`, 20_000);
    await menu(page, "qa5_coll", "Generate DDL");
    await page.waitFor(`/CREATE TABLE/.test(${editorText})`, 15_000);
    const ddl = await page.eval(editorText);
    assert.match(ddl, /COLLATE\s+Latin1_General_CS_AS/i, `the collation of name is gone:\n${ddl}`);
  } finally {
    DBS.sqlserver.exec(DROP);
  }
}
