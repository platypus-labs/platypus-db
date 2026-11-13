// qa4-xfer-1 (medium). The SQL INSERT export of a ClickHouse Map or Tuple column cannot be run again.
// Steps: ClickHouse table (id Int32, mp Map(String,Int32), tup Tuple(Int32,String), arr Array(Int32))
// with one row; editor › SELECT * › Export… › SQL INSERT; run the statements into a copy of the table.
// Expected: the statements insert the row (a Map literal is {'k':1}, a Tuple (1,'x')), so the copy
// holds what the source holds, as it does for the Array column.
// Actual: the Map and the Tuple are written as quoted JSON text ('{"k":1}', '{"1":1,"2":"x"}');
// ClickHouse answers "Cannot parse quoted string ... while converting '{"k":1}' to Map(String, Int32)"
// and "Cannot parse input: expected '(' before: '{"1":0,...}'". Nothing is inserted.
// Code: the SQL writer of the export (crates/platypus-transfer, literal for non-scalar kinds)
// quotes every nested value as text; only Arrays happen to parse from a string.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { db, dbQuiet, DBS, exportResult, openEditor, runScript, saveConnections, skipUnless } from "../transfer-helpers.mjs";

export const skip = () => skipUnless("clickhouse");

export function setup({ dir }) {
  saveConnections(dir, ["clickhouse"]);
  db("clickhouse", "DROP TABLE IF EXISTS qa4x_map; DROP TABLE IF EXISTS qa4x_map_copy");
  db("clickhouse", "CREATE TABLE qa4x_map (id Int32, mp Map(String,Int32), tup Tuple(Int32,String), arr Array(Int32)) ENGINE=MergeTree ORDER BY id");
  db("clickhouse", "CREATE TABLE qa4x_map_copy AS qa4x_map");
  db("clickhouse", "INSERT INTO qa4x_map VALUES (1, {'k':1}, (1,'x'), [1,2,3]), (2, {}, (0,''), [])");
}

export default async function (page, { dir }) {
  try {
    await openEditor(page, DBS.clickhouse.name);
    assert.equal(await runScript(page, "SELECT * FROM qa4x_map ORDER BY id"), "2 rows");
    const file = join(dir, "map.sql");
    assert.match(await exportResult(page, file, { format: "sql", table: "qa4x_map_copy" }), /^Exported 2 rows/);
    const errors = [];
    for (const statement of readFileSync(file, "utf8").split(";\n").filter((s) => s.trim())) {
      try {
        db("clickhouse", statement);
      } catch (e) {
        errors.push(e.message.split("\n").slice(0, 2).join(" "));
      }
    }
    assert.deepEqual(errors, []);
    assert.equal(db("clickhouse", "SELECT count() FROM qa4x_map_copy"), "2");
  } finally {
    dbQuiet("clickhouse", "DROP TABLE IF EXISTS qa4x_map; DROP TABLE IF EXISTS qa4x_map_copy");
  }
}
