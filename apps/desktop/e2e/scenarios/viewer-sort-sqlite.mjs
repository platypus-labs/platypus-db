// The data viewer keeps the query of its editor: a WHERE typed there survives the header sort
// (which only changes the top-level ORDER BY), and "Count rows" counts that query. Another
// table's query typed there is never edited as the viewer's table, even once sorted.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E viewer sort";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const statusSpan = `${visibleTab}?.querySelector('.statusbar span')`;
const status = `(${statusSpan}?.textContent ?? '')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "sort.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT);
           INSERT INTO items VALUES (1, 1, 'c'), (2, 2, 'b'), (3, 1, 'a'), (4, 2, 'd'), (5, 1, 'e');
           CREATE TABLE others (code INTEGER PRIMARY KEY, label TEXT);
           INSERT INTO others VALUES (1, 'x'), (2, 'y');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000011"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;

/** Empties the status first: "3 rows" may be the old state. The run's end is a new text with
 *  Cancel disabled again; "Executing…" is only its start. */
async function afterRun(page, action) {
  await page.eval(`(() => { const s = ${statusSpan}; if (s) s.textContent = ''; })()`);
  await action();
  await page.waitFor(
    `(() => { const s = ${status}; return ${cancel}?.disabled === true && s !== '' && !s.includes('Executing'); })()`,
    15_000,
  );
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("items")} !== undefined`);
  await page.eval(`${treeRow("items")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${status} === '5 rows'`, 15_000);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.viewer-bar input')`), null, "no WHERE bar");

  // A WHERE typed in the editor.
  const query = "SELECT * FROM items WHERE grp = 1";
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(query);
  await page.waitFor(`${editorText} === ${JSON.stringify(query)}`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
  assert.equal(await page.eval(status), "3 rows");

  // A header click sorts, a second reverses it, a third removes the sort: the WHERE stays.
  const grid = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  const header = () => page.click(grid.x + 40, grid.y + 14);
  await afterRun(page, header);
  assert.match(await page.eval(editorText), /^SELECT \* FROM items WHERE grp = 1\nORDER BY \w+ ASC$/);
  assert.equal(await page.eval(status), "3 rows");
  await afterRun(page, header);
  assert.match(await page.eval(editorText), /^SELECT \* FROM items WHERE grp = 1\nORDER BY \w+ DESC$/);
  await afterRun(page, header);
  assert.equal(await page.eval(editorText), query);
  await afterRun(page, header);
  assert.match(await page.eval(editorText), /WHERE grp = 1\nORDER BY \w+ ASC$/);

  // "Count rows" counts the filtered query, not the whole table.
  await page.eval(`${visibleTab}.querySelector('.viewer-bar button[aria-label="Count rows"]').click()`);
  await page.waitFor(`${visibleTab}.querySelector('.viewer-bar .muted')?.textContent === '3 rows'`, 15_000);

  // With a script, the sort rewrites the statement that ran, not the first one.
  const script = "SELECT * FROM items WHERE grp = 2;\nSELECT * FROM items WHERE grp = 1";
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(script);
  await page.waitFor(`${editorText} === ${JSON.stringify(script)}`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
  assert.equal(await page.eval(status), "3 rows");
  await afterRun(page, header);
  const sorted = await page.eval(editorText);
  assert.ok(/\nORDER BY id (ASC|DESC)$/.test(sorted) && sorted.startsWith(script), sorted);

  // A statement that is not a read-only query is never rewritten: the headers do not sort.
  const update = "UPDATE items SET name = name WHERE id = 1 RETURNING id, name";
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(update);
  await page.waitFor(`${editorText} === ${JSON.stringify(update)}`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
  assert.equal(await page.eval(status), "1 row");
  await header();
  await page.sleep(500);
  assert.equal(await page.eval(editorText), update);

  // Another query typed in the viewer does not read the viewer's table, sorted or not: the rows
  // of a join stay read-only instead of being edited as rows of items.
  const join = "SELECT o.code, o.label FROM others o JOIN items i ON i.id = o.code";
  const editBar = `${visibleTab}.querySelector('.edit-bar:not([hidden])')`;
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(join);
  await page.waitFor(`${editorText} === ${JSON.stringify(join)}`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
  assert.equal(await page.eval(status), "2 rows");
  await afterRun(page, header);
  assert.match(await page.eval(editorText), /\nORDER BY code ASC$/);
  assert.equal(await page.eval(status), "2 rows");
  // The viewer's edit bar would show at once; give the grid's edit target time to load.
  await page.sleep(500);
  assert.equal(await page.eval(`${editBar} === null`), true, "the sorted join is not editable as items");
}
