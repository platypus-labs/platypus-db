// SQL-20. SQL Server datetime2(7) and datetimeoffset(7) lose their 7th fractional digit:
// '2024-01-02T03:04:05.1234567' shows as 2024-01-02 03:04:05.123456 (the values are read into
// microseconds). time(7) keeps all digits, so the grid shows different precisions.
// Steps: SQL Server, Ctrl+Enter on the SELECT below, copy the grid with headers.
// Expected: 2024-01-02 03:04:05.1234567.
// Actual: 2024-01-02 03:04:05.123456.
// Code: crates/platypus-driver-mssql columns: datetime2 mapped to Timestamp(Microsecond).
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, runStatement, saveConnections, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("mssql");
export const setup = ({ dir }) => saveConnections(dir, ["mssql"]);

export default async function (page) {
  await openEditor(page, DB.mssql.name);
  await runStatement(page, "SELECT CAST('2024-01-02T03:04:05.1234567' AS datetime2(7)) AS dt2");
  assert.match(await gridTsv(page), /^dt2\n2024-01-02 03:04:05\.1234567/);
}
