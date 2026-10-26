// Compare with previous run: after the same statement runs again on changed rows, the button
// opens the rows added, removed and changed (matched by the table's key), with the old value of
// a changed cell; another statement forgets the copy and the button goes.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E diff";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const statusSpan = `${visibleTab}?.querySelector('.statusbar span')`;
const status = `(${statusSpan}?.textContent ?? '')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const compareButton = `[...(${visibleTab}?.querySelectorAll('.statusbar button') ?? [])].find(b => b.textContent === 'Compare with previous run')`;
const diffTab = `[...document.querySelectorAll('.diff-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  const path = join(dir, "diff.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT);
           INSERT INTO items VALUES (1, 1, 'a'), (2, 2, 'b'), (3, 1, 'c'), (4, 2, 'd'), (5, 1, 'e');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000034"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

async function afterRun(page, action) {
  await page.eval(`(() => { const s = ${statusSpan}; if (s) s.textContent = ''; })()`);
  await action();
  await page.waitFor(
    `(() => { const s = ${status}; return ${cancel}?.disabled === true && s !== '' && !s.includes('Executing'); })()`,
    15_000,
  );
}

async function runQuery(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("items")} !== undefined`);
  await page.eval(`${treeRow("items")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${status} === '5 rows'`, 15_000);
  assert.equal(await page.eval(`${compareButton} ?? null`), null, "nothing to compare yet");

  // One row changed, one removed, one added; then the same statement again.
  const db = new DatabaseSync(join(dir, "diff.db"), { timeout: 5_000 });
  db.exec("UPDATE items SET name = 'zz' WHERE id = 2; DELETE FROM items WHERE id = 5; INSERT INTO items VALUES (6, 2, 'f');");
  db.close();
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
  assert.equal(await page.eval(status), "5 rows");
  await page.waitFor(`${compareButton}?.disabled === false`);
  await page.eval(`${compareButton}.click()`);
  await page.waitFor(`${diffTab}?.querySelector('.diff-counts')?.textContent === '+1 −1 ~1'`, 10_000);
  assert.equal(await page.eval(`${diffTab}.querySelector('.diff-summary .muted').textContent`), "Rows matched by the table's key");
  const changed = `${diffTab}.querySelector('tr.changed td.diff-cell-changed')`;
  assert.equal(await page.eval(`${changed}.textContent`), "zz");
  assert.equal(await page.eval(`${changed}.title`), "Before: b");
  assert.equal(await page.eval(`${diffTab}.querySelector('tr.removed td:nth-child(2)').textContent`), "5");

  // Another statement: the copy is forgotten.
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith('items')).click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`);
  await runQuery(page, "SELECT name FROM items");
  assert.equal(await page.eval(`${compareButton} ?? null`), null);
}
