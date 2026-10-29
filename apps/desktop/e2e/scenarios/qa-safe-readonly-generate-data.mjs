// BUG SAFE-01: "Generate data…" writes into a read-only connection.
// Steps: a SQLite connection saved with "Read-only" (read_only = true) and a table t with 2 rows;
// navigator > t > Generate data… > Generate.
// Expected: like the editor, the grid, the table editor and Drop/Truncate/Rename, nothing
// writes: Generate data is not offered on a read-only connection (restricted actions are hidden
// since 76d60b06), and t keeps its 2 rows.
// Actual: Generate is enabled and inserts 100 rows ("Added 100 rows"). GenerateDataDialog
// checks needsProductionConfirm() but never connection.read_only.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ro.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b');");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000421"
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
  await page.waitFor(`${row("t")} !== undefined`, 15_000);
  await page.eval(`${row("t")}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  // A read-only connection takes no rows: the table's menu offers no data generation.
  const items = await page.eval(`[...document.querySelectorAll('.context-menu button')].map(b => b.textContent)`);
  if (shots) await page.screenshot(join(shots, "qa-safe-readonly-generate-data.png"));
  assert.ok(!items.some((t) => t.startsWith("Generate data")), `Generate data offered on a read-only connection: ${items.join(" | ")}`);
  const db = new DatabaseSync(join(dir, "ro.db"));
  const n = db.prepare("SELECT count(*) AS n FROM t").get().n;
  db.close();
  assert.equal(n, 2, `the read-only connection now has ${n} rows in t`);
}
