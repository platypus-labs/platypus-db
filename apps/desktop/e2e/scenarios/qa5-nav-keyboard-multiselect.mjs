// QA5-NAV-1 (medium). The navigator cannot select several objects from the keyboard.
// FEATURES: "Ctrl/Shift+click selects several objects, for example to get the DDL of many tables
// at once". With the keyboard, Shift+Down / Shift+Up just move the single selection (and Ctrl+A
// selects nothing), so a keyboard-only user cannot reach the multi-object actions (DDL of
// many tables, drop several), which an explorer-style tree offers with Shift+arrows.
// Steps: expand a SQLite connection with tables; click `tab_01`; press Shift+Down twice.
// Expected: tab_01, tab_02 and tab_03 are selected. Actual: only tab_03 (the focused row).
// Code: apps/desktop/src/components/Navigator.tsx `onTreeKey` (`go` calls
//   `setSelected(new Set([rows[index].key]))` whatever the modifiers).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { row, saveConnections } from "../qa-nav-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "k.db"));
  for (let i = 1; i <= 6; i++) db.exec(`CREATE TABLE tab_0${i} (id INTEGER PRIMARY KEY);`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a5a01"
name = "QA Lite"
driver = "sqlite"
database = '${join(dir, "k.db")}'
save_password = false
`);
}

export default async function (page) {
  await page.eval(`${row("QA Lite")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("tab_01")} !== undefined`, 15_000);
  await page.eval(`${row("tab_01")}.querySelector('.tree-label').id = 'e2e-t1'`);
  await page.clickSel("#e2e-t1");
  await page.key("ArrowDown", "ArrowDown", 40, 8);
  await page.key("ArrowDown", "ArrowDown", 40, 8);
  await page.sleep(300);
  const selected = await page.eval(`[...document.querySelectorAll('.tree-row.selected .tree-label')].map(l => l.textContent)`);
  assert.deepEqual(selected, ["tab_01", "tab_02", "tab_03"]);
}
