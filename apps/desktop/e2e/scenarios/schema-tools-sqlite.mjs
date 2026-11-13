// The schema tools on throwaway SQLite databases: the ER diagram of a schema with a note,
// the schema check with its fix opened in the editor, and the comparison with another
// database opening the script that makes it like this one.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { revealItem } from "../menu-sub.mjs";

const NAME = "Shop db";
const TARGET = "Shop copy";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const erd = `[...document.querySelectorAll('.erd')].find(t => t.style.display !== 'none')`;
const editorText = `[...document.querySelectorAll('.cm-content')].map(e => e.textContent).join('\\n')`;

export function setup({ dir }) {
  const shop = new DatabaseSync(join(dir, "shop.db"));
  shop.exec(`CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL);
             CREATE TABLE log (line TEXT);`);
  shop.close();
  const copy = new DatabaseSync(join(dir, "copy.db"));
  copy.exec(`CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE);`);
  copy.close();
  const connection = (n, name, file) => `
[[connections]]
id = "00000000-0000-4000-8000-00000000003${n}"
name = "${name}"
driver = "sqlite"
database = '${join(dir, file)}'
save_password = false
`;
  writeFileSync(join(dir, "connections.toml"), `version = 1\n${connection(1, NAME, "shop.db")}${connection(2, TARGET, "copy.db")}`);
}

/** Runs the context-menu entry starting with `title` on the navigator row `label`. */
async function menu(page, label, title) {
  await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  assert.ok(await revealItem(page, title), `no menu item ${title}`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(title)})).click()`,
  );
}

/** Clicks the button whose text starts with `text` inside the element `scope`. */
const click = (page, scope, text) =>
  page.eval(`[...(${scope}).querySelectorAll('button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(text)})).click()`);

export default async function (page, { shots }) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("main")} !== undefined`, 15_000);

  // The diagram: three tables, one foreign key line, then a note.
  await menu(page, "main", "ER diagram");
  await page.waitFor(`${erd}?.querySelectorAll('.erd-table').length === 3`, 15_000);
  assert.equal(await page.eval(`${erd}.querySelectorAll('path[marker-start]').length`), 1);
  await click(page, erd, "Add note");
  await page.waitFor(`document.querySelector('.modal textarea') !== null`);
  await page.clickSel(".modal textarea");
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("Orders by customer");
  await click(page, "document.querySelector('.modal')", "Save");
  await page.waitFor(`${erd}.querySelector('.erd-note')?.textContent.includes('Orders by customer')`);
  if (shots) await page.screenshot(join(shots, "erd.png"));

  // The check: the key of orders has no index, log has no primary key.
  await menu(page, "main", "Check schema");
  await page.waitFor(`document.querySelectorAll('.lint-findings li').length > 0`, 15_000);
  const findings = await page.eval(`document.querySelector('.lint-findings').textContent`);
  assert.match(findings, /Foreign key without index/);
  assert.match(findings, /No primary key/);
  await click(page, "document.querySelector('.modal.schema-lint')", "Open 1 fix");
  await page.waitFor(`document.querySelector('.modal.schema-lint') === null`);
  await page.waitFor(`(${editorText}).includes('CREATE INDEX')`, 10_000);

  // The comparison: the copy lacks orders and log.
  await menu(page, "main", "Compare with");
  await page.waitFor(`document.querySelector('.modal.schema-compare select')?.value !== undefined && document.querySelectorAll('.modal.schema-compare select')[1]?.value === 'main'`, 15_000);
  await click(page, "document.querySelector('.modal.schema-compare')", "Compare");
  await page.waitFor(`document.querySelector('.modal.schema-compare') === null`, 15_000);
  await page.waitFor(`(${editorText}).includes('CREATE TABLE main.orders')`, 10_000);
  assert.match(await page.eval(editorText), /CREATE TABLE main\.log/);
}
