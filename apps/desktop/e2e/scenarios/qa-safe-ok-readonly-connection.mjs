// Regression guard (passes): a connection saved as read-only (read_only = true, not the tab's
// lock) stops changes where the app checks it: the editor shows its notice and runs nothing,
// the data viewer says the grid is read-only, the table editor's Save is disabled, and the
// navigator's Truncate, Drop and Rename show their statement with the button disabled.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { openEditor, readOnlyNotice, runOrGuard, treeRow, visibleTab } from "../qa-safe-db.mjs";

const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ro.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b');");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000551"
name = "RO db"
driver = "sqlite"
database = '${join(dir, "ro.db")}'
save_password = false
read_only = true
`,
  );
}

async function menu(page, label, item) {
  await page.eval(`${treeRow(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})).click()`);
}

export default async function (page, { dir }) {
  const count = () => {
    const db = new DatabaseSync(join(dir, "ro.db"));
    try {
      return db.prepare("SELECT count(*) AS n FROM t").get().n;
    } finally {
      db.close();
    }
  };
  for (const sql of ["DELETE FROM t WHERE id = 1", "DROP TABLE t"]) {
    await openEditor(page, "RO db");
    const r = await runOrGuard(page, sql, { readOnly: true });
    assert.match(r, /^read-only: RO db is read-only and this may modify data/, `${sql}: ${r}`);
  }
  assert.equal(count(), 2);

  await page.eval(`${treeRow("RO db")}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("t")} !== undefined`, 15_000);
  await page.eval(`${treeRow("t")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${editBar}.startsWith('Read-only')`, 15_000);

  for (const [item, button] of [["Truncate", "Truncate"], ["Drop", "Drop"], ["Rename", "Rename"]]) {
    await menu(page, "t", item);
    const modal = `document.querySelector('.modal.sql-action')`;
    await page.waitFor(`${modal}?.textContent.includes('RO db is read-only: this statement cannot be run.')`, 10_000);
    assert.equal(await page.eval(`[...${modal}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(button)}).disabled`), true, item);
    await page.eval(`[...${modal}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Cancel').click()`);
    await page.waitFor(`${modal} === null`);
  }

  await menu(page, "t", "Edit table");
  const editor = `[...document.querySelectorAll('.table-editor')].find(t => t.style.display !== 'none')`;
  await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 2`, 10_000);
  assert.equal(await page.eval(`${editor}.querySelector('.object-header button.primary').disabled`), true);
  assert.equal(count(), 2);
  assert.equal(await page.eval(readOnlyNotice), "");
}
