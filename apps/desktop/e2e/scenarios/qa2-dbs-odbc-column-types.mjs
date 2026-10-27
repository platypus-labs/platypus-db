// DBS-14 (medium): through the generic ODBC driver, SQL Server's date/time and money columns
// get wrong types in the navigator and the table editor: datetime2(7) is shown as
// "datetime2(27)", datetimeoffset(7) as "datetimeoffset(34)", time(7) as "time(16)" and money
// as "money(19,4)". The number is the ODBC column size (characters of the text form), not the
// type's precision; a table editor save that touches such a column writes the invalid type
// (SQL Server: "Specified scale 27 is invalid").
//
// Steps: SQL Server (docker profile sqlserver) through ODBC with Windows' "SQL Server" driver;
// qa_dbs.t (id int PRIMARY KEY, d datetime2(7), o datetimeoffset, tm time(7), m money);
// navigator › t › Edit table….
// Expected: datetime2(7), datetimeoffset(7), time(7), money (as the native driver shows).
// Actual: datetime2(27), datetimeoffset(34), time(16), money(19,4).
// Suspected: crates/platypus-driver-odbc/src/meta.rs (the type name built from SQLColumns'
// TYPE_NAME + COLUMN_SIZE / DECIMAL_DIGITS for every type that has a size).
import assert from "node:assert/strict";
import { MSSQL_DROP, mssqlSchema, needsPorts, odbcConnection, openPath, sh } from "../qa-dbs.mjs";
import { menu } from "../qa-xfer.mjs";
import { editor } from "../qa-nav-table-editor.mjs";

export const skip = needsPorts(51433);

export function setup({ dir }) {
  mssqlSchema(`CREATE TABLE qa_dbs.t (id int PRIMARY KEY, d datetime2(7), o datetimeoffset, tm time(7), m money);`);
  odbcConnection(dir);
}

export default async function (page) {
  try {
    await openPath(page, ["QA ODBC", "qa_dbs", "Tables", "t"]);
    await menu(page, "t", "Edit table", null, ["QA ODBC", "qa_dbs", "Tables"]);
    await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 5`, 15_000);
    const types = await page.eval(`[...${editor}.querySelectorAll('tbody tr')].map(r => r.querySelectorAll('td')[1].querySelector('input').value)`);
    assert.deepEqual(types.slice(1), ["datetime2(7)", "datetimeoffset(7)", "time(7)", "money"]);
  } finally {
    sh("mssql", MSSQL_DROP);
  }
}
