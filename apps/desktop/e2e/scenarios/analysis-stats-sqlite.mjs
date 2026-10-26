// Column statistics: resting the mouse on a result column's header shows its NULLs, distinct
// values, extremes, most frequent values and over how many rows; moving away hides them.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E column stats";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const statusSpan = `${visibleTab}?.querySelector('.statusbar span')`;
const status = `(${statusSpan}?.textContent ?? '')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const profile = `${visibleTab}?.querySelector('.column-profile')`;

export function setup({ dir }) {
  const path = join(dir, "stats.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT);
           INSERT INTO items VALUES (1, 1, 'a'), (2, 2, 'b'), (3, 1, 'a'), (4, 2, NULL), (5, 1, 'c');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000031"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

/** Empties the status first: "5 rows" may be the old state. The run's end is a new text with
 *  Cancel disabled again. */
async function afterRun(page, action) {
  await page.eval(`(() => { const s = ${statusSpan}; if (s) s.textContent = ''; })()`);
  await action();
  await page.waitFor(
    `(() => { const s = ${status}; return ${cancel}?.disabled === true && s !== '' && !s.includes('Executing'); })()`,
    15_000,
  );
}

/** Types `sql` as the editor's whole text and runs it. */
async function runQuery(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("items")} !== undefined`);
  await page.eval(`${treeRow("items")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${status} === '5 rows'`, 15_000);
  await runQuery(page, "SELECT name, grp FROM items");
  assert.equal(await page.eval(status), "5 rows");

  const grid = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  // Nothing until the mouse rests on a header.
  assert.equal(await page.eval(`${profile} ?? null`), null);
  await page.mouse("mouseMoved", grid.x + 40, grid.y + 14);
  await page.waitFor(`${profile}?.textContent.includes('of all 5 rows') === true`, 5_000);
  const text = await page.eval(`${profile}.textContent`);
  assert.match(text, /^name/);
  assert.match(text, /NULL values\s*20%/);
  assert.match(text, /Distinct\s*3/);
  assert.match(text, /Minimum\s*a/);
  assert.match(text, /Maximum\s*c/);
  assert.deepEqual(
    await page.eval(`[...${profile}.querySelectorAll('.column-profile-top span')].map(s => s.textContent)`),
    ["a", "2", "b", "1", "c", "1"],
  );

  // Away from the header, the box goes.
  await page.mouse("mouseMoved", grid.x + 40, grid.y + 120);
  await page.waitFor(`${profile} == null`);
}
