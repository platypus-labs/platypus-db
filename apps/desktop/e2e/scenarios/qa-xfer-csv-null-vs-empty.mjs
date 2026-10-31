// XFER-23 (low). The CSV export writes NULL and the empty string the same way.
// Steps: run `SELECT NULL AS a, '' AS b`; Export… › CSV.
// Expected: the two values can be told apart, e.g. the empty string quoted ("") — which the
// app's own import reads back as '' once XFER-12 is fixed — or a "NULL as" option, as DBeaver
// has.
// Actual: the line is `,` — both fields empty; a round trip through CSV turns '' into NULL (or
// NULL into ''). The Excel export drops the empty string as well (an empty cell).
// Suspected: crates/platypus-transfer/src/lib.rs:276-282 (with_null(String::new()) and the
// writer's default quoting).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { exportResult, openEditor, runScript, sqliteOnly } from "../qa-xfer.mjs";

export const setup = ({ dir }) => sqliteOnly(dir);

export default async function (page, { dir }) {
  await openEditor(page, "QA SQLite");
  assert.equal(await runScript(page, "SELECT NULL AS a, '' AS b"), "1 row");
  assert.match(await exportResult(page, join(dir, "out.csv"), { format: "csv", source: "loaded" }), /^Exported 1 row\b/);
  const [, values] = readFileSync(join(dir, "out.csv"), "utf8").split(/\r?\n/);
  const [a, b] = values.split(",");
  assert.notEqual(a, b, `NULL written as ${JSON.stringify(a)}, '' as ${JSON.stringify(b)}`);
}
