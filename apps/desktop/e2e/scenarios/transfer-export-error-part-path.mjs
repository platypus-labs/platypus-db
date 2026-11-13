// XFER-25 (low). A failed export names the internal ".part" file instead of the user's file.
// Steps: run any query; Export… › CSV to a file in a folder that does not exist (or is not
// writable).
// Expected: "Export failed: <the chosen file>: <reason>".
// Actual: "Export failed: …\nope\x.csv.part: The system cannot find the path specified" — a
// name the user never chose, from the temporary file the export writes first.
// Suspected: crates/platypus-transfer/src/lib.rs:268-271 (io_error(&partial) on create).
import assert from "node:assert/strict";
import { join } from "node:path";
import { exportResult, openEditor, runScript, sqliteOnly } from "../transfer-helpers.mjs";

export const setup = ({ dir }) => sqliteOnly(dir);

export default async function (page, { dir }) {
  await openEditor(page, "QA SQLite");
  assert.equal(await runScript(page, "SELECT 1 AS a"), "1 row");
  const message = await exportResult(page, join(dir, "nope", "x.csv"), { source: "loaded" });
  assert.match(message, /^Export failed/);
  assert.doesNotMatch(message, /\.part\b/);
}
