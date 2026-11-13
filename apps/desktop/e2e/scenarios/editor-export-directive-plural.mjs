// qa4-sql-8 (low, wording). The `@export file.csv` directive reports its result as
// "Exported 1 rows" for a single row (the editor's own status says "1 row" and "1 row affected").
// Steps: SQLite editor, run the script `@export <file>.csv` newline `SELECT 1 AS a`.
// Expected: "Exported 1 row".
// Actual: "Exported 1 rows" (status and result tab).
// Code: the @export result message (apps/desktop/src/components/QueryTab.tsx or the Rust
//   directive handler) is not pluralised like the other counts.
import assert from "node:assert/strict";
import { join } from "node:path";
import { openEditor, resultTabs, runScript, saveConnections } from "../editor-helpers.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });

export default async function (page, { dir }) {
  await openEditor(page, "QA SQLite");
  const file = join(dir, "q4out.csv").replaceAll("\\", "/");
  const status = await runScript(page, `@export ${file}\nSELECT 1 AS a`);
  const tabs = await page.eval(resultTabs);
  assert.ok(!/\b1 rows\b/.test(`${status} ${tabs.join(" ")}`), `"1 rows" in "${status}" / ${JSON.stringify(tabs)}`);
}
