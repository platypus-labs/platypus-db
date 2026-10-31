// QA5 safe: a PRAGMA that writes runs on a production connection without the confirmation.
//
// Steps: SQLite connection of type Production; in its SQL editor run `PRAGMA user_version = 42`
// (also `PRAGMA writable_schema = 1`, which lets a session edit the schema table by hand).
// Expected: like every change on production ("every change is listed and confirmed before it
// runs", docs/FEATURES.md, Transactions and safety): the "Changes on Prod db (production)" dialog
// lists the statement. Actual: no dialog; the statement runs ("0 rows affected") and the session
// shows "Transaction open". The same hole is there for `SET GLOBAL ...` / `SET PASSWORD` on MySQL
// and MariaDB and `ANALYZE`, which only the session-local meaning of PostgreSQL's SET justifies.
// Code: crates/platypus-sql/src/analysis.rs `OTHER` ("SET", "PRAGMA", "ANALYZE"... give
// StatementKind::Other) and guards/guards.ts `check`, which asks on production only for the
// "dml" and "ddl" kinds.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const treeRow = (label) => `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "prod.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a');");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1\n[[connections]]\nid = "00000000-0000-4000-8000-000000000e51"\nname = "Prod db"\ndriver = "sqlite"\ndatabase = '${join(dir, "prod.db")}'\nsave_password = false\nconnection_type = "prod"\n`,
  );
}

export default async function (page) {
  await page.eval(`${treeRow("Prod db")}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  const sql = "PRAGMA user_version = 42";
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.sleep(1500);
  const guard = await page.eval(`document.querySelector('.modal.guard')?.textContent ?? null`);
  assert.ok(guard !== null, "PRAGMA user_version = 42 ran on production without the confirmation");
  assert.match(guard, /PRAGMA user_version = 42/);
}
