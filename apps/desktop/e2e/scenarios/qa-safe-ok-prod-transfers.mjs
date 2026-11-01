// Regression guard (passes): on a production connection, the writes that do not go through
// the SQL editor ask first, and Cancel leaves the database as it was: fake data, a pasted
// CSV imported as a new table, and a schema copied into it from another connection. The
// navigator's Drop shows its statement before running it.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { revealItem } from "../menu-sub.mjs";

const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const rows = (label) => `[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const guard = `document.querySelector('.modal.guard')`;

export function setup({ dir }) {
  const prod = new DatabaseSync(join(dir, "prod.db"));
  prod.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b');");
  prod.close();
  const dev = new DatabaseSync(join(dir, "dev.db"));
  dev.exec("CREATE TABLE u (id INTEGER PRIMARY KEY); INSERT INTO u VALUES (1);");
  dev.close();
  writeFileSync(join(dir, "pasted.csv"), "city,people\nMilano,1\nTorino,2\n");
  const c = (id, name, file, extra) => `
[[connections]]
id = "00000000-0000-4000-8000-00000000041${id}"
name = "${name}"
driver = "sqlite"
database = '${join(dir, file)}'
save_password = false
${extra}
`;
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${c(1, "Prod db", "prod.db", 'connection_type = "prod"')}${c(2, "Dev db", "dev.db", "")}`);
}

/** Opens the context menu of the `index`-th navigator row labelled `label` and picks `item`. */
async function menu(page, label, item, index = 0) {
  await page.eval(`${rows(label)}[${index}].querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await revealItem(page, item);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})).click()`);
}

/** Clicks `action` in `modal`, expects the production confirmation and cancels it. */
async function askedAndCancelled(page, modal, action) {
  await page.waitFor(`${button(modal, action)} && !${button(modal, action)}.disabled`, 15_000);
  await page.eval(`${button(modal, action)}.click()`);
  await page.waitFor(`${guard} !== null`);
  assert.match(await page.eval(`${guard}.textContent`), /Prod db \(production\)/);
  await page.eval(`${button(guard, "Cancel")}.click()`);
  await page.waitFor(`${guard} === null`);
}

const tables = (dir) => {
  const db = new DatabaseSync(join(dir, "prod.db"));
  try {
    return {
      names: db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all().map((r) => r.name),
      rows: db.prepare("SELECT count(*) AS n FROM t").get().n,
    };
  } finally {
    db.close();
  }
};

export default async function (page, { dir }) {
  await page.waitFor(`${row("Prod db")} !== undefined`);
  await page.eval(`${row("Prod db")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("t")} !== undefined`, 15_000);

  const gen = `document.querySelector('.modal.generate-data')`;
  await menu(page, "t", "Generate data");
  await page.waitFor(`${gen}?.querySelectorAll('select').length === 2`, 15_000);
  await askedAndCancelled(page, gen, "Generate");
  await page.eval(`${button(gen, "Cancel")}.click()`);
  await page.waitFor(`${gen} === null`);

  await page.eval(`window.__platypus.answer("clipboard", ${JSON.stringify(join(dir, "pasted.csv"))})`);
  const imp = `document.querySelector('.modal.import-data')`;
  await menu(page, "main", "Paste CSV as a new table");
  await page.waitFor(`${imp}?.querySelectorAll('.import-preview tbody tr').length === 2`, 15_000);
  await askedAndCancelled(page, imp, "Import");
  await page.eval(`${button(imp, "Cancel")}.click()`);
  await page.waitFor(`${imp} === null`);

  await page.eval(`${row("Dev db")}.querySelector('.twisty').click()`);
  await page.waitFor(`${rows("main")}.length === 2`, 15_000);
  const mig = `document.querySelector('.modal.migrate')`;
  await menu(page, "main", "Copy schema to a database", 1);
  await page.waitFor(`${mig}?.querySelector('select')?.selectedOptions[0]?.textContent === 'Prod db'`, 15_000);
  await askedAndCancelled(page, mig, "Copy");
  await page.eval(`${button(mig, "Cancel")}.click()`);
  await page.waitFor(`${mig} === null`);

  // Drop shows its statement and waits for the button.
  await menu(page, "t", "Drop");
  const action = `document.querySelector('.modal.sql-action')`;
  await page.waitFor(`${action}?.querySelector('pre')?.textContent.includes('DROP TABLE t')`, 10_000);
  await page.eval(`${button(action, "Cancel")}.click()`);
  await page.waitFor(`${action} === null`);

  assert.deepEqual(tables(dir), { names: ["t"], rows: 2 });
}
