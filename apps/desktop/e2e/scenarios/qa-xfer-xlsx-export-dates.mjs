// XFER-28 (low). The Excel export writes dates and timestamps as text cells.
// Steps: on PostgreSQL run `SELECT DATE '2024-01-31' AS d, TIMESTAMP '2024-01-31 10:30' AS ts`;
// Export… › Excel workbook.
// Expected: date cells (a number with a date format), so Excel sorts, filters and computes
// on them — as DBeaver's XLSX export does.
// Actual: inline strings "2024-01-31" and "2024-01-31T10:30:00": Excel shows them left-aligned
// as text, and the ISO "T" form is not even recognised when retyped.
// Suspected: crates/platypus-transfer/src/text.rs:243-267 (xlsx_row writes numbers and
// booleans natively, everything else with write_string).
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DBS, exportResult, openEditor, runScript, saveConnections, skipUnless, zipEntry } from "../qa-xfer.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page, { dir }) {
  await openEditor(page, DBS.pg.name);
  assert.equal(await runScript(page, "SELECT DATE '2024-01-31' AS d, TIMESTAMP '2024-01-31 10:30' AS ts"), "1 row");
  assert.match(await exportResult(page, join(dir, "out.xlsx"), { format: "xlsx", source: "loaded" }), /^Exported 1 rows/);
  const sheet = zipEntry(join(dir, "out.xlsx"), "xl/worksheets/sheet1.xml");
  const a2 = /<c r="A2"[^>]*>.*?<\/c>/.exec(sheet)?.[0] ?? "";
  assert.doesNotMatch(a2, /t="inlineStr"|t="s"/, `A2 is text: ${a2}`);
}
