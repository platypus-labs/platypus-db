// qa5-xfer-2 (low). The Excel export writes a floating-point number from 2^53 up (6.02e23, 1e20,
// 1e300) as a text cell, so the sheet cannot sum, sort or chart it.
// Steps: run `SELECT 6.02e23 AS avogadro, 1e20 AS big, 1.5e15 AS ok`; Export... > Excel.
// Expected: all three are numeric cells (`<c r="A2"><v>6.02E+23</v></c>`), as 1.5e15 is: a double
// is already what Excel stores, nothing is lost.
// Actual: avogadro and big are `t="inlineStr"` cells holding "6.02e23" and "1e20" (shown left
// aligned as text, with the green "number stored as text" marker in Excel).
// Code: crates/platypus-transfer/src/text.rs xlsx_row: `n.abs() < XLSX_MAX_EXACT` is applied to
// ValueKind::Float and Real too; the limit only matters for integers and decimals, whose digits
// an f64 cannot keep, not for a value that already is an f64.
import assert from "node:assert/strict";
import { join } from "node:path";
import { exportResult, openEditor, runScript, sqliteOnly, zipEntry } from "../transfer-helpers.mjs";

export const setup = ({ dir }) => sqliteOnly(dir);

export default async function (page, { dir }) {
  await openEditor(page, "QA SQLite");
  assert.equal(await runScript(page, "SELECT 6.02e23 AS avogadro, 1e20 AS big, 1.5e15 AS ok"), "1 row");
  assert.match(await exportResult(page, join(dir, "o.xlsx"), { format: "xlsx", source: "loaded" }), /^Exported 1 row\b/);
  const row = zipEntry(join(dir, "o.xlsx"), "xl/worksheets/sheet1.xml").match(/<row r="2">.*?<\/row>/)[0];
  assert.ok(!row.includes("inlineStr"), `text cells in the data row: ${row}`);
}
