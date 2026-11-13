// Bug NAV: on DuckDB, Open details (and Generate DDL) of an ENUM type fails with "Deprecated
// lambda arrow (->) detected. Please transition to the new lambda syntax…": the bundled DuckDB
// rejects the `l -> …` lambda the DDL query uses, so the Properties and DDL sections show
// only that error.
//
// Cause: the `ObjectKind::Type` query in `object_ddl`, crates/platypus-driver-duckdb/src/meta.rs
// (`list_transform(labels, l -> …)`); the new syntax is `lambda l: …`. The same query also
// writes the type and macro names unquoted.
//
// Steps: create `CREATE TYPE qa_nav_mood AS ENUM ('sad', 'ok')` in a DuckDB file, Refresh,
// Types › qa_nav_mood › Open details.
// Expected: `CREATE TYPE qa_nav_mood AS ENUM ('sad', 'ok');`. Actual: the deprecation error.
import assert from "node:assert/strict";
import { join } from "node:path";
import { childRow, menu, row, runSql, saveConnections, visibleTab } from "../navigator-helpers.mjs";

const objTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a0044"
name = "QA Duck"
driver = "duckdb"
database = '${join(dir, "enum.duckdb")}'
save_password = false
`);
}

export default async function (page) {
  await menu(page, "QA Duck", "New SQL script");
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await runSql(page, "CREATE TYPE qa_nav_mood AS ENUM ('sad', 'ok')");
  await page.eval(`${row("QA Duck")}.querySelector('.twisty').click()`);
  await page.waitFor(`${childRow("main", "Types")} !== undefined`, 15_000);
  await page.eval(`${childRow("main", "Types")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("qa_nav_mood")} !== undefined`);
  await menu(page, "qa_nav_mood", "Open details");
  await page.waitFor(`${objTab}?.querySelector('.object-title strong')?.textContent === 'qa_nav_mood'`, 10_000);
  await page.eval(`[...${objTab}.querySelectorAll('.object-sections button')].find(b => b.textContent === 'DDL').click()`);
  await page.waitFor(`${objTab}.querySelector('.object-ddl .cm-content, .notice.error') !== null`, 10_000);
  const error = await page.eval(`${objTab}.querySelector('.notice.error')?.textContent ?? null`);
  assert.equal(error, null);
  assert.match(await page.eval(`${objTab}.querySelector('.object-ddl .cm-content').textContent`), /CREATE TYPE qa_nav_mood AS ENUM \('sad', 'ok'\);/);
}
