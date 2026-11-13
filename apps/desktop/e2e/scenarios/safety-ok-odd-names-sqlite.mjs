// Regression guard (passes): generated SQL quotes odd names and values. A SQLite table and
// column whose names hold double and single quotes, brackets, backticks, a semicolon and a
// comment marker: the data viewer opens it, an edited cell (a value with the same characters
// and a DROP TABLE after a semicolon), a deleted and an added row save to that table only,
// and "Export data…" writes the values back unchanged.
import assert from "node:assert/strict";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { revealItem } from "../menu-sub.mjs";

const TABLE = "qa \"q' [b] `t` ; -- x";
const COL = "c \"q' [b] `t`";
const VALUE = "it's \"x\" `y` [z]; DROP TABLE victim; --";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const q = (s) => `"${s.replaceAll('"', '""')}"`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "names.db"));
  db.exec(`CREATE TABLE ${q(TABLE)} (id INTEGER PRIMARY KEY, ${q(COL)} TEXT);
    INSERT INTO ${q(TABLE)} VALUES (1, 'a'), (2, 'b'), (3, 'c');
    CREATE TABLE victim (id INTEGER); INSERT INTO victim VALUES (1);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000521"
name = "Names db"
driver = "sqlite"
database = '${join(dir, "names.db")}'
save_password = false
`,
  );
}

async function editCell(page, text) {
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.key("a", "KeyA", 65, 2);
  await page.type(text);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow("Names db")} !== undefined`);
  await page.eval(`${treeRow("Names db")}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow(TABLE)} !== undefined`, 15_000);
  await page.eval(`${treeRow(TABLE)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${status} === '3 rows'`, 15_000);
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);

  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("ArrowRight", "ArrowRight", 39);
  await editCell(page, VALUE);
  await page.key("Delete", "Delete", 46);
  await page.key("Insert", "Insert", 45);
  await editCell(page, "new'row");
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Preview SQL"]').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  await page.waitFor(`${editBar}.includes('to edit a cell') && ${status} === '3 rows'`, 15_000);

  const db = new DatabaseSync(join(dir, "names.db"));
  assert.deepEqual(
    db.prepare(`SELECT id, ${q(COL)} AS v FROM ${q(TABLE)} ORDER BY id`).all().map((r) => ({ ...r })),
    [{ id: 1, v: VALUE }, { id: 3, v: "c" }, { id: 4, v: "new'row" }],
  );
  assert.equal(db.prepare("SELECT count(*) AS n FROM victim").get().n, 1);
  db.close();

  const out = join(dir, "out");
  mkdirSync(out);
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(out)})`);
  await page.eval(`${treeRow(TABLE)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await revealItem(page, "Export data");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Export data')).click()`);
  const tables = `document.querySelector('.modal.export-tables')`;
  await page.waitFor(`[...(${tables}?.querySelectorAll('button') ?? [])].some(b => b.textContent.trim() === 'Export to folder…' && !b.disabled)`, 15_000);
  await page.eval(`[...${tables}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Export to folder…').click()`);
  await page.waitFor(`${tables}.textContent.includes('Exported 3 rows')`, 20_000);
  const [file] = readdirSync(out);
  assert.equal(
    readFileSync(join(out, file), "utf8").replaceAll("\r\n", "\n"),
    `id,"c ""q' [b] \`t\`"\n1,"it's ""x"" \`y\` [z]; DROP TABLE victim; --"\n3,c\n4,new'row\n`,
  );
}
