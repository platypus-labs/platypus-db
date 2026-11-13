// QA5-NAV-2 (low). PageDown / PageUp do nothing in the navigator tree: the focus and the
// selection stay on the row, while the arrow keys, Home and End work. In a schema with
// hundreds of tables a keyboard user can only go by arrows or End.
// Steps: expand a SQLite connection with 60 tables; click `tab_01`; press PageDown.
// Expected: the focus and selection move down by about a screenful. Actual: no change.
// Code: apps/desktop/src/components/Navigator.tsx `onTreeKey` (no case for PageUp/PageDown).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { row, saveConnections } from "../navigator-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "k.db"));
  for (let i = 10; i < 70; i++) db.exec(`CREATE TABLE tab_${i} (id INTEGER PRIMARY KEY);`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a5a02"
name = "QA Lite"
driver = "sqlite"
database = '${join(dir, "k.db")}'
save_password = false
`);
}

export default async function (page) {
  await page.eval(`${row("QA Lite")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("tab_10")} !== undefined`, 15_000);
  await page.eval(`${row("tab_10")}.querySelector('.tree-label').id = 'e2e-t1'`);
  await page.clickSel("#e2e-t1");
  await page.key("PageDown", "PageDown", 34, 0);
  await page.sleep(300);
  const selected = await page.eval(`[...document.querySelectorAll('.tree-row.selected .tree-label')].map(l => l.textContent)`);
  assert.ok(selected.length === 1 && selected[0] !== "tab_10", `PageDown left the selection on ${selected.join(",")}`);
}
