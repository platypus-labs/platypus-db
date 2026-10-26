// BUG SAFE-01: "Generate data…" writes into a read-only connection.
// Steps: a SQLite connection saved with "Read-only" (read_only = true) and a table t with 2 rows;
// navigator > t > Generate data… > Generate.
// Expected: like the editor, the grid, the table editor and Drop/Truncate/Rename, the dialog
// refuses: Generate disabled with the "is read-only" notice, and t keeps its 2 rows.
// Actual: Generate is enabled and inserts 100 rows ("Added 100 rows"). GenerateDataDialog
// checks needsProductionConfirm() but never connection.read_only.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const gen = `document.querySelector('.modal.generate-data')`;

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
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Generate data')).click()`);
  await page.waitFor(`${gen}?.querySelectorAll('select').length === 2`, 15_000);
  if (shots) await page.screenshot(join(shots, "qa-safe-readonly-generate-data.png"));

  const enabled = await page.eval(`${button(gen, "Generate")}?.disabled === false`);
  if (enabled) {
    await page.eval(`${button(gen, "Generate")}.click()`);
    await page.waitFor(`/Added|Nothing was added|read-only/.test(${gen}.textContent)`, 20_000);
  }
  const db = new DatabaseSync(join(dir, "ro.db"));
  const n = db.prepare("SELECT count(*) AS n FROM t").get().n;
  db.close();
  assert.equal(n, 2, `the read-only connection now has ${n} rows in t`);
  assert.equal(enabled, false, "Generate is enabled on a read-only connection");
  assert.match(await page.eval(`${gen}.textContent`), /read-only/);
}
