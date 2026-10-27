// SQL-27 (low). ClickHouse arrays with NULL elements show the NULL as nothing: [1,2,NULL]
// appears as "[1, 2, ]", the same as an array ending with an empty string. Tuples show as
// "{1: 1, 2: a}" instead of (1, 'a').
// Steps: ClickHouse, Ctrl+Enter on `SELECT [1, 2, NULL] AS arr`.
// Expected: [1, 2, NULL].
// Actual: [1, 2, ].
// Code: the grid's formatting of Arrow list values (null items rendered as "").
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, runStatement, saveConnections, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("clickhouse");
export const setup = ({ dir }) => saveConnections(dir, ["clickhouse"]);

export default async function (page) {
  await openEditor(page, DB.clickhouse.name);
  await runStatement(page, "SELECT [1, 2, NULL] AS arr");
  assert.equal(await gridTsv(page), "arr\n[1, 2, NULL]");
}
