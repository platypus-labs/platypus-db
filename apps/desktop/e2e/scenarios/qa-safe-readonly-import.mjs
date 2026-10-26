// BUG SAFE-02: importing data writes into a read-only connection.
// Steps: a SQLite connection saved with "Read-only" (read_only = true); navigator > main >
// "Paste CSV as a new table…" with a 2-row CSV (the same ImportDataDialog serves "Import
// data…" and "Copy to a database…") > Import.
// Expected: Import disabled with the "is read-only" notice, no table created.
// Actual: "Imported 2 rows", the table `pasted` is created in the read-only database.
// ImportDataDialog checks needsProductionConfirm() but never target.connection.read_only.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const imp = `document.querySelector('.modal.import-data')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ro.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY)");
  db.close();
  writeFileSync(join(dir, "pasted.csv"), "city,people\nMilano,1\nTorino,2\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000422"
name = "RO db"
driver = "sqlite"
database = '${join(dir, "ro.db")}'
save_password = false
read_only = true
`,
  );
}

export default async function (page, { dir, shots }) {
  await page.waitFor(`${row("RO db")} !== undefined`);
  await page.eval(`${row("RO db")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("main")} !== undefined`, 15_000);
  await page.eval(`window.__platypus.answer("clipboard", ${JSON.stringify(join(dir, "pasted.csv"))})`);
  await page.eval(`${row("main")}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Paste CSV as a new table')).click()`);
  await page.waitFor(`${imp}?.querySelectorAll('.import-preview tbody tr').length === 2`, 15_000);
  if (shots) await page.screenshot(join(shots, "qa-safe-readonly-import.png"));

  const enabled = await page.eval(`${button(imp, "Import")}?.disabled === false`);
  if (enabled) {
    await page.eval(`${button(imp, "Import")}.click()`);
    await page.waitFor(`/Imported|failed|read-only/i.test(${imp}.textContent)`, 20_000);
  }
  const db = new DatabaseSync(join(dir, "ro.db"));
  const names = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all().map((r) => r.name);
  db.close();
  assert.deepEqual(names, ["t"], "a table was created in the read-only database");
  assert.equal(enabled, false, "Import is enabled on a read-only connection");
}
