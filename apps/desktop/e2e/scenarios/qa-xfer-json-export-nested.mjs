// XFER-17 (medium). The JSON export writes JSON values, arrays and documents as strings.
// Steps: on PostgreSQL run `SELECT '{"a": 1}'::jsonb AS j, ARRAY[1, 2] AS arr`; Export… ›
// JSON (array). (MongoDB: `db.c.find()` on documents with an array and a sub-document gives
// the same.)
// Expected: [{"j":{"a":1},"arr":[1,2]}] — a jsonb column, a PostgreSQL array, a MongoDB array
// or document are JSON already, and DBeaver writes them as such.
// Actual: [{"j":"{\"a\": 1}","arr":"{1,2}"}]: each is an escaped string (the array in
// PostgreSQL's own text form), so a consumer has to parse twice, or cannot.
// Suspected: crates/platypus-transfer/src/lib.rs:171-191 (as_text() turns every non-scalar
// Arrow type, and json text, into a string).
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { DBS, exportResult, openEditor, runScript, saveConnections, skipUnless } from "../qa-xfer.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page, { dir }) {
  await openEditor(page, DBS.pg.name);
  assert.equal(await runScript(page, `SELECT '{"a": 1}'::jsonb AS j, ARRAY[1, 2] AS arr`), "1 row");
  assert.match(await exportResult(page, join(dir, "out.json"), { format: "json", source: "loaded" }), /^Exported 1 rows/);
  const text = readFileSync(join(dir, "out.json"), "utf8");
  assert.deepEqual(JSON.parse(text), [{ j: { a: 1 }, arr: [1, 2] }], text);
}
