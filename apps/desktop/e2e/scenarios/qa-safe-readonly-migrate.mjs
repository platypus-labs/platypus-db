// BUG SAFE-03: "Copy schema to a database…" offers a read-only connection as the target and
// writes into it.
// Steps: "Dev db" (table u, 2 rows) and "RO db" saved with "Read-only" (read_only = true);
// navigator > Dev db > main > Copy schema to a database… : RO db is the preselected (and only)
// target; Copy.
// Expected: read-only connections are not offered as targets (or Copy is disabled with the
// "is read-only" notice); RO db is unchanged.
// Actual: "Copied 2 rows": table u is created and filled in RO db. MigrateDialog lists every
// connection and checks needsProductionConfirm(target) but never target.read_only.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { revealItem } from "../menu-sub.mjs";

const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const mig = `document.querySelector('.modal.migrate')`;

export function setup({ dir }) {
  const ro = new DatabaseSync(join(dir, "ro.db"));
  ro.exec("CREATE TABLE t (id INTEGER PRIMARY KEY)");
  ro.close();
  const dev = new DatabaseSync(join(dir, "dev.db"));
  dev.exec("CREATE TABLE u (id INTEGER PRIMARY KEY); INSERT INTO u VALUES (1), (2);");
  dev.close();
  const c = (id, name, file, extra) => `
[[connections]]
id = "00000000-0000-4000-8000-00000000042${id}"
name = "${name}"
driver = "sqlite"
database = '${join(dir, file)}'
save_password = false
${extra}
`;
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${c(3, "Dev db", "dev.db", "")}${c(4, "RO db", "ro.db", "read_only = true")}`);
}

export default async function (page, { dir, shots }) {
  await page.waitFor(`${row("Dev db")} !== undefined`);
  await page.eval(`${row("Dev db")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("main")} !== undefined`, 15_000);
  await page.eval(`${row("main")}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await revealItem(page, "Copy schema to a database");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Copy schema to a database')).click()`);
  await page.waitFor(`${mig}?.querySelector('select') != null`, 15_000);
  await page.sleep(500);
  if (shots) await page.screenshot(join(shots, "qa-safe-readonly-migrate.png"));

  const target = await page.eval(`${mig}.querySelector('select').selectedOptions[0]?.textContent ?? null`);
  const enabled = await page.eval(`${button(mig, "Copy")}?.disabled === false`);
  if (target === "RO db" && enabled) {
    await page.eval(`${button(mig, "Copy")}.click()`);
    await page.waitFor(`/Copied|failed|read-only/i.test(${mig}.textContent)`, 20_000);
  }
  const db = new DatabaseSync(join(dir, "ro.db"));
  const names = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all().map((r) => r.name);
  db.close();
  assert.deepEqual(names, ["t"], "the schema was copied into the read-only database");
  assert.ok(!(target === "RO db" && enabled), "a read-only connection is offered as the target with Copy enabled");
}
