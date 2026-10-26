// Group D, P2 and P3 on a throwaway SQLite database: parameters and variables, client
// commands, snippets, function completion, results in a new tab, terminal mode, outline,
// linter, spell check, folding and Ctrl+click to open an object.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E SQLite";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const options = `[...document.querySelectorAll('.cm-tooltip-autocomplete li')].map(li => li.textContent)`;
const resultTabs = `[...${visibleTab}.querySelectorAll('.result-tab')].map(t => t.textContent)`;
const button = (label) => `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === ${JSON.stringify(label)})`;
// The run is over when Cancel is disabled again.
const idle = `${button("Cancel")}?.disabled === true`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "e2e.db"));
  db.exec("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO customers (name) VALUES ('ada');");
  db.close();
  writeFileSync(join(dir, "inc.sql"), "SELECT 7 AS seven;\nSELECT 8 AS eight;\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000001"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "e2e.db")}'
save_password = false
`,
  );
}

async function setEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
}

async function runStatement(page) {
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`(() => { const s = ${status}; return s !== '' && !s.includes('Executing'); })() && ${idle}`, 15_000);
  return page.eval(status);
}

/** Runs the script and waits for a result tab containing `last`, which the previous run did
 *  not have. */
async function runScript(page, last) {
  await page.eval(`${button("Script")}.click()`);
  await page.waitFor(`${resultTabs}.some(t => t.includes(${JSON.stringify(last)})) && ${idle}`, 15_000);
  return page.eval(resultTabs);
}

/** The centre of `word` in the editor, for clicking on it. */
const wordAt = (word) => `(() => {
  const walker = document.createTreeWalker(${visibleTab}.querySelector('.cm-content'), NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const i = n.textContent.indexOf(${JSON.stringify(word)});
    if (i < 0) continue;
    const r = document.createRange();
    r.setStart(n, i + 1);
    r.setEnd(n, i + 2);
    const b = r.getBoundingClientRect();
    return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
  }
  return null;
})()`;

export default async function (page, { dir, shots }) {
  const connectionRow = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;
  await page.waitFor(`${connectionRow} !== undefined`);
  await page.eval(`${connectionRow}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`);

  // :name asks its value once and keeps it as a variable; ${name} then uses it as typed.
  await setEditor(page, "SELECT :n + 1 AS v");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.params-dialog input') !== null`);
  await page.type("41");
  await page.eval(`[...document.querySelectorAll('.params-dialog button')].find(b => b.textContent === 'Run').click()`);
  await page.waitFor(`${status} === '1 row' && ${idle}`, 15_000);
  await setEditor(page, "SELECT ${n} * 2 AS w");
  assert.equal(await runStatement(page), "1 row");
  assert.equal(await page.eval(`document.querySelector('.params-dialog')`), null, "a known variable is not asked");
  await page.eval(`${button("Variables")}.click()`);
  await page.waitFor(`document.querySelector('.variables-panel .variable-name')?.textContent === 'n'`);
  assert.equal(await page.eval(`document.querySelector('.variables-panel .variable-row input').value`), "41");
  await page.eval(`${button("Variables")}.click()`);

  // Client commands run in the app, one result each; @include adds a file's statements.
  await setEditor(page, `@set who = 'x'\n@echo hello \${who}\n@include ${join(dir, "inc.sql")}\nSELECT 1 AS one;`);
  const tabs = await runScript(page, "6. 1 row");
  assert.deepEqual(
    tabs.map((t) => t.replace(/×$/, "")),
    ["1. who = 'x'", "2. hello 'x'", "3. Included 2 statements", "4. 1 row", "5. 1 row", "6. 1 row"],
  );
  // The wheel scrolls the result tabs sideways, once they overflow their bar.
  const bar = `${visibleTab}.querySelector('.result-tabs')`;
  await page.eval(`${bar}.style.maxWidth = '200px'`);
  const r = await page.rect(".query-tab:not([style*='none']) .result-tabs");
  await page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: r.x + 50, y: r.y + r.h / 2, deltaX: 0, deltaY: 100 });
  await page.waitFor(`${bar}.scrollLeft > 0`);
  await page.eval(`${bar}.style.maxWidth = ''`);

  // @export writes the next statement's rows to a file instead of the grid.
  const out = join(dir, "out.csv");
  await setEditor(page, `@export ${out}\nSELECT 1 AS a, 'b' AS b;`);
  const exported = await runScript(page, "Exported");
  assert.match(exported[1] ?? "", /Exported 1 rows/);
  assert.match(readFileSync(out, "utf8"), /^a,b\r?\n1,b/);

  // Ctrl+\ runs into a new result tab; the × closes one.
  await setEditor(page, "SELECT 1");
  await runStatement(page);
  await setEditor(page, "SELECT 2");
  await page.key("\\", "Backslash", 220, CTRL);
  await page.waitFor(`${resultTabs}.length === 2 && ${idle}`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.result-tab-close').click()`);
  await page.waitFor(`${resultTabs}.length === 0`);

  // Terminal mode prints results as text.
  await page.eval(`${button("Terminal")}.click()`);
  await page.waitFor(`${button("Terminal")}.classList.contains('toggled') && document.querySelector('.terminal-output') !== null`);
  await setEditor(page, "SELECT name FROM customers");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`(document.querySelector('.terminal-output')?.textContent ?? '').includes('(1 row)')`, 15_000);
  assert.match(await page.eval(`document.querySelector('.terminal-output').textContent`), /name\n-+\n ada/);
  await page.eval(`${button("Terminal")}.click()`);

  // Snippets expand with Tab; functions complete with their signature.
  await setEditor(page, "selc");
  await page.key("Tab", "Tab", 9);
  await page.waitFor(`${editorText} === 'SELECT COUNT(*)\\nFROM table;'`);
  await setEditor(page, "SELECT coal");
  await page.key(" ", "Space", 32, CTRL);
  await page.waitFor(`${options}.some(o => o.startsWith('coalesce'))`, 10_000);
  // The function's description shows beside the list, not clipped by it.
  const selected = `document.querySelector('.cm-tooltip-autocomplete li[aria-selected]')?.textContent ?? ''`;
  await page.sleep(150);
  for (let i = 0; i < 20 && !(await page.eval(selected)).startsWith("coalesce"); i++) {
    await page.key("ArrowDown", "ArrowDown", 40);
    await page.sleep(50);
  }
  assert.match(await page.eval(selected), /^coalesce/);
  await page.waitFor(`(() => {
    const info = document.querySelector('.cm-completionInfo');
    if (!info || info.textContent.trim() === '') return false;
    const r = info.getBoundingClientRect();
    return r.width > 0 && info.contains(document.elementFromPoint(r.left + r.width / 2, r.top + Math.min(r.height / 2, 10)));
  })()`);
  if (shots) await page.screenshot(join(shots, "completion-info.png"));
  await page.key("Escape", "Escape", 27);

  // The outline lists the statements, client commands too.
  await setEditor(page, "SELECT 1;\nSELECT 2;\n@echo x");
  await page.eval(`${button("Outline")}.click()`);
  await page.waitFor(`document.querySelectorAll('.outline-row').length === 3`);
  await page.eval(`${button("Outline")}.click()`);

  // The linter marks an unknown column and a join without a condition.
  await setEditor(page, "SELECT c.nme FROM customers c");
  await page.waitFor(`${visibleTab}.querySelector('.cm-lintRange-error') !== null`, 10_000);
  await setEditor(page, "SELECT 1 FROM customers a JOIN customers b");
  await page.waitFor(`${visibleTab}.querySelector('.cm-lintRange-warning') !== null`, 10_000);

  // Spell check covers comments only; statements fold from the gutter.
  await setEditor(page, "-- a comment\nSELECT 1\n  FROM customers");
  await page.waitFor(`${visibleTab}.querySelector('.cm-spellcheck')?.getAttribute('spellcheck') === 'true'`);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.cm-content').getAttribute('spellcheck')`), "false");
  assert.ok(await page.eval(`${visibleTab}.querySelector('.cm-foldGutter') !== null`));

  // Ctrl+click on a table name opens the table.
  const at = await page.eval(wordAt("customers"));
  assert.ok(at, "customers in the editor");
  await page.click(at.x, at.y, { modifiers: CTRL });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('customers')`, 10_000);
}
