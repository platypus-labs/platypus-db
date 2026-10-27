// What the scenarios on a SQLite file share: the saved connection, the visible query tab and
// running statements in it.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "./cdp.mjs";

export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const statusSpan = `${visibleTab}?.querySelector('.statusbar span')`;
export const status = `(${statusSpan}?.textContent ?? '')`;
// The SQL editor only: the field above a result is a CodeMirror too.
export const editorText = `[...(${visibleTab}?.querySelector('.editor')?.querySelectorAll('.cm-line') ?? [])].map(l => l.textContent).join('\\n')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
export const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

/** Creates the SQLite file `file` in `dir` with `sql`, and saves a connection to it. */
export function sqliteConnection(dir, { file, id, name, sql }) {
  const path = join(dir, file);
  const db = new DatabaseSync(path);
  db.exec(sql);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

/** Opens `table` of the connection `name` from the navigator and waits for its `rows`. */
export async function openTable(page, name, table, rows) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow(table)} !== undefined`);
  await page.eval(`${treeRow(table)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${status} === ${JSON.stringify(rows)}`, 15_000);
}

/** Empties the status first: the old text may be the same. The run's end is a new text with
 *  Cancel disabled again. */
async function afterRun(page, action) {
  await page.eval(`(() => { const s = ${statusSpan}; if (s) s.textContent = ''; })()`);
  await action();
  await page.waitFor(
    `(() => { const s = ${status}; return ${cancel}?.disabled === true && s !== '' && !s.includes('Executing'); })()`,
    15_000,
  );
}

/** Runs the editor's text of the visible tab again. */
export async function rerun(page) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
}

/** Types `sql` as the editor's whole text and runs it. */
export async function runQuery(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
}
