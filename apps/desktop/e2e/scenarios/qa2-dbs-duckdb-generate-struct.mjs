// DBS-11 (medium): "Generate data…" adds nothing to a DuckDB table with a STRUCT or UNION
// column whose fields include an integer: the type name "STRUCT(a INTEGER, b VARCHAR)"
// contains "INT", so the column gets "Whole number", and DuckDB rejects the run ("Unimplemented
// type for cast (INTEGER -> STRUCT(a INTEGER, b VARCHAR))"). The same happens to Trino's
// row("a" integer, …). Arrays and maps already get NULL; structs, unions and rows do not.
//
// Steps: DuckDB file; CREATE TABLE g (id INTEGER PRIMARY KEY, s STRUCT(a INTEGER, b VARCHAR),
// un UNION(n INTEGER, s VARCHAR)); navigator › g › Generate data…, keep the suggestions, 5 rows.
// Expected: s and un left NULL (no generator fits), "Added 5 rows.".
// Actual: both get "Whole number"; "Nothing was added: Unimplemented type for cast …".
// Suspected: apps/desktop/src/transfer/generators.ts:37 (`isInteger` matches "int" anywhere)
// and :59 (the composite types list lacks struct, union and row).
import assert from "node:assert/strict";
import { join } from "node:path";
import { conns, openEditor, openPath, run, treeMenu } from "../qa-dbs.mjs";
import { generate } from "../qa-xfer.mjs";

export function setup({ dir }) {
  conns(dir, [["QA Duck", { duckdb: join(dir, "g.duckdb") }]]);
}

export default async function (page) {
  await openEditor(page, "QA Duck", null);
  await run(page, "CREATE TABLE g (id INTEGER PRIMARY KEY, s STRUCT(a INTEGER, b VARCHAR), un UNION(n INTEGER, s VARCHAR));", true);
  await treeMenu(page, "QA Duck", "Refresh");
  await openPath(page, ["QA Duck", "main", "Tables", "g"]);
  const { kinds, out } = await generate(page, "g", ["QA Duck", "main"], 5);
  assert.match(out, /Added 5 rows/, `${out} (generators ${kinds})`);
}
