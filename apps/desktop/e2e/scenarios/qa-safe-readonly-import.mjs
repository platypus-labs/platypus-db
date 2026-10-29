// BUG SAFE-02: importing data writes into a read-only connection.
// Steps: a SQLite connection saved with "Read-only" (read_only = true); navigator > main >
// "Paste CSV as a new table…" with a 2-row CSV (the same ImportDataDialog serves "Import
// data…" and "Copy to a database…") > Import.
// Expected: no import offered on a read-only connection (restricted actions are hidden since
// 76d60b06; the dialog also refuses with the "is read-only" notice), no table created.
// Actual: "Imported 2 rows", the table `pasted` is created in the read-only database.
// ImportDataDialog checks needsProductionConfirm() but never target.connection.read_only.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

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
  // A read-only connection takes no rows: the menu offers no import at all.
  const items = await page.eval(`[...document.querySelectorAll('.context-menu button')].map(b => b.textContent)`);
  if (shots) await page.screenshot(join(shots, "qa-safe-readonly-import.png"));
  for (const item of ["Paste CSV as a new table", "Import data into a new table", "Import data"]) {
    assert.ok(!items.some((t) => t.startsWith(item)), `${item} offered on a read-only connection: ${items.join(" | ")}`);
  }
  const db = new DatabaseSync(join(dir, "ro.db"));
  const names = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all().map((r) => r.name);
  db.close();
  assert.deepEqual(names, ["t"], "a table was created in the read-only database");
}
