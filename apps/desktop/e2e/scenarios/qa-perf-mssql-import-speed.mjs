// BUG (medium, slow): importing into SQL Server runs at ~4,500 rows/s. 1,000,000 CSV rows took
// 220 s (PostgreSQL: 17 s), 100,000 fake rows 21.5 s (PostgreSQL: 2.8 s); the app is idle
// (10 s of CPU in 220 s): the time goes in SQL Server compiling each 500-row INSERT … VALUES
// of literals (ROWS_PER_INSERT in crates/platypus-transfer/src/import.rs; fake data goes the
// same way). A bulk load (TDS bulk insert, which tiberius offers) or parameterized batches
// would be an order of magnitude faster; DBeaver uses batched prepared statements.
//   Steps: "Paste CSV as a new table" on dbo with a 100,000-row CSV, Import.
//   Expected: done within 8 s.  Actual: ~22 s.
// Needs SQL Server (docker compose -f docker/compose.yml --profile sqlserver up -d).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { MSSQL, expand, mssqlEntry, needsPort, openEditor, rowMenu, run, saveConnections, treeRow } from "../perf.mjs";

export const skip = needsPort(MSSQL.port, "SQL Server");
const TABLE = "qa_perf_import";
const ROWS = 100_000;
const LIMIT_MS = 8000;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

export function setup({ dir }) {
  const lines = ["id,name,amount,created,note"];
  for (let i = 1; i <= ROWS; i++) lines.push(`${i},name ${i},${(i % 1000) / 10},2024-01-01 10:00:00,${"x".repeat(30)}${i}`);
  writeFileSync(join(dir, `${TABLE}.csv`), `${lines.join("\n")}\n`);
  saveConnections(dir, mssqlEntry());
}

export default async function (page, { dir }) {
  await expand(page, MSSQL.name, MSSQL.password);
  await openEditor(page, MSSQL.name);
  await run(page, `DROP TABLE IF EXISTS dbo.${TABLE}`);
  await page.waitFor(`${treeRow("platypus")} !== undefined`, 20_000);
  if ((await page.eval(`${treeRow("platypus")}.getAttribute('aria-expanded')`)) !== "true") {
    await page.eval(`${treeRow("platypus")}.querySelector('.twisty').click()`);
  }
  await page.waitFor(`${treeRow("dbo")} !== undefined`, 20_000);
  try {
    await page.eval(`window.__platypus.answer('clipboard', ${JSON.stringify(join(dir, `${TABLE}.csv`))})`);
    await rowMenu(page, "dbo", "Paste CSV as a new table");
    const dialog = `document.querySelector('.modal.import-data')`;
    await page.waitFor(`${dialog}?.querySelectorAll('.import-preview tbody tr').length > 0`, 30_000);
    await page.waitFor(`${button(dialog, "Import")} && !${button(dialog, "Import")}.disabled`, 15_000);
    const started = performance.now();
    await page.eval(`${button(dialog, "Import")}.click()`);
    await page.waitFor(`/Imported [\\d.,]+ rows/.test(${dialog}.textContent) || /error|failed/i.test(${dialog}.textContent)`, 300_000);
    const took = performance.now() - started;
    const text = await page.eval(`${dialog}.textContent`);
    console.log(`  ${ROWS} rows into SQL Server: ${Math.round(took)} ms`);
    assert.match(text, /Imported 100[.,]?000 rows/);
    assert.ok(took < LIMIT_MS, `importing ${ROWS} rows took ${Math.round(took)} ms`);
    await page.eval(`${button(dialog, "Close")}.click()`);
  } finally {
    await page.eval(`document.querySelector('.modal.import-data') && [...document.querySelectorAll('.modal.import-data button')].find(b => b.textContent.trim() === 'Close')?.click()`);
    await run(page, `DROP TABLE IF EXISTS dbo.${TABLE}`);
  }
}
