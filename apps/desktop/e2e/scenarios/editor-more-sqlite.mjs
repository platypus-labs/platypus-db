// Group D, P2 and P3 on a throwaway SQLite database: parameters and variables, client
// commands, snippets, function completion, results in a new tab, terminal mode, outline,
// linter, spell check, folding and Ctrl+click to open an object. Then the four in more depth:
// the linter's messages on hover (unknown column, join without condition, SELECT * only on a
// production connection, and quiet once fixed), the spell check of comments, of comments and
// strings, and off, the outline's rows (kinds, headlines, a click selecting the statement, the
// empty state) and multiple cursors (add above/below, select next occurrence, Escape).
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL } from "../cdp.mjs";

const NAME = "E2E SQLite";
const PROD = "E2E prod";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
// The editor's lines, not those of a result's SQL field (`.sql-line`, with its placeholder).
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].filter(l => !l.closest('.sql-line')).map(l => l.textContent).join('\\n')`;
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

[[connections]]
id = "00000000-0000-4000-8000-000000000002"
name = "${PROD}"
driver = "sqlite"
database = '${join(dir, "e2e.db")}'
connection_type = "prod"
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

  await editorInDepth(page);
}

const connectionRow = (name) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(name)})`;

/** A new editor on the connection `name`. */
async function newEditor(page, name) {
  const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${connectionRow(name)}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-connection')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length === ${before + 1} && ${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
}

/** Moves the mouse over the first element matching `selector` of the editor and returns the text
 *  of the lint tooltip that opens. */
async function lintTip(page, selector) {
  const at = await page.eval(`(() => {
    const el = ${visibleTab}.querySelector(${JSON.stringify(selector)});
    if (!el) return null;
    const b = el.getBoundingClientRect();
    return { x: b.x + Math.min(b.width / 2, 20), y: b.y + b.height / 2 };
  })()`);
  assert.ok(at, `${selector} in the editor`);
  await page.mouse("mouseMoved", at.x - 30, at.y - 30);
  await page.mouse("mouseMoved", at.x, at.y);
  await page.waitFor(`document.querySelector('.cm-tooltip-lint') !== null`, 10_000);
  return page.eval(`document.querySelector('.cm-tooltip-lint').textContent`);
}

async function lintInDepth(page) {
  // A typo in a column: an error that says which column and table; fixed, it goes.
  await setEditor(page, "SELECT c.nme FROM customers c");
  await page.waitFor(`${visibleTab}.querySelector('.cm-lintRange-error') !== null`, 10_000);
  assert.match(await lintTip(page, ".cm-lintRange-error"), /Column nme does not exist in customers\./);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.cm-lint-marker') !== null`), true, "a mark in the gutter");
  await setEditor(page, "SELECT c.name FROM customers c");
  await page.waitFor(`${visibleTab}.querySelector('.cm-lintRange-error') === null`, 10_000);

  // A join without a condition warns; on a development connection SELECT * does not.
  await setEditor(page, "SELECT * FROM customers a JOIN customers b");
  await page.waitFor(`${visibleTab}.querySelector('.cm-lintRange-warning') !== null`, 10_000);
  const tip = await lintTip(page, ".cm-lintRange-warning");
  assert.match(tip, /JOIN without ON/);
  assert.doesNotMatch(tip, /production/);
  assert.equal(await page.eval(`${visibleTab}.querySelectorAll('.cm-lintRange-warning').length`), 1, "SELECT * is not a finding here");

  // The production connection warns about SELECT *.
  await newEditor(page, PROD);
  await setEditor(page, "SELECT * FROM customers");
  await page.waitFor(`${visibleTab}.querySelector('.cm-lintRange-warning') !== null`, 10_000);
  assert.match(await lintTip(page, ".cm-lintRange-warning"), /SELECT \* on a production connection/);
  await setEditor(page, "SELECT name FROM customers");
  await page.waitFor(`${visibleTab}.querySelector('.cm-lintRange-warning') === null`, 10_000);
}

async function spellInDepth(page) {
  // Comments only by default: every marked range is inside a comment, the string is not.
  await setEditor(page, "-- teh comment\nSELECT 'strng' AS word");
  await page.waitFor(`${visibleTab}.querySelector('.cm-spellcheck') !== null`);
  const marked = () => page.eval(`[...${visibleTab}.querySelectorAll('.cm-spellcheck')].map(e => e.textContent)`);
  assert.ok((await marked()).every((t) => t.startsWith("--")), JSON.stringify(await marked()));

  // The setting: comments and strings, then off.
  const pick = async (value) => {
    await page.key(",", "Comma", 188, CTRL);
    await page.waitFor(`document.querySelector('.modal.settings') !== null`);
    await page.settingsTab("SQL editor");
    const select = `document.querySelector('.modal.settings .setting[data-key="editor.spellcheck"] select')`;
    await page.waitFor(`${select} !== null`);
    await page.eval(`(() => {
      const el = ${select};
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
      el.dispatchEvent(new Event('change', { bubbles: true }));
    })()`);
    await page.key("Escape", "Escape", 27);
    await page.waitFor(`document.querySelector('.modal.settings') === null`);
  };
  await pick("all");
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.cm-spellcheck')].some(e => e.textContent.includes("'strng'"))`, 10_000);
  await pick("off");
  await page.waitFor(`${visibleTab}.querySelector('.cm-spellcheck') === null`, 10_000);
  await pick("comments");
  await page.waitFor(`${visibleTab}.querySelector('.cm-spellcheck') !== null`, 10_000);
  assert.ok((await marked()).every((t) => t.startsWith("--")));
}

async function outlineInDepth(page) {
  const outline = `${visibleTab}.querySelector('.outline-panel')`;
  const rows = `[...document.querySelectorAll('.outline-row')].map(r => [r.querySelector('.outline-kind').textContent, r.querySelector('.outline-text').textContent])`;
  const toggle = `${visibleTab}.querySelector('.toolbar button[aria-label="Outline"]')`;
  // Nothing in the editor: the empty state.
  await setEditor(page, "");
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`${outline}?.textContent.includes('No statements')`, 10_000);
  // A comment before a statement is not its headline; a long line is cut; a command is marked @.
  const long = `SELECT ${"a_very_long_column_name, ".repeat(6)}1 FROM customers`;
  await setEditor(page, `-- the first\nSELECT 5;\n${long};\n@echo hi`);
  await page.waitFor(`document.querySelectorAll('.outline-row').length === 3`, 10_000);
  const listed = await page.eval(rows);
  assert.equal(listed[0][1], "1. SELECT 5");
  assert.equal(listed[1][1].length, "2. ".length + 80);
  assert.equal(listed[2][0], "@");
  // A click selects the statement in the editor.
  await page.eval(`document.querySelectorAll('.outline-row')[0].click()`);
  await page.waitFor(`/^SELECT 5;?$/.test(window.getSelection().toString())`, 10_000);
  // The list follows the text.
  await setEditor(page, "SELECT 1;");
  await page.waitFor(`document.querySelectorAll('.outline-row').length === 1`, 10_000);
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`${outline} === null`);
}

/** Waits for `n` cursors in the editor; on a miss, says how many there are. */
async function cursorsAre(page, n) {
  const count = `${visibleTab}.querySelectorAll('.cm-cursor').length`;
  try {
    await page.waitFor(`${count} === ${n}`, 8_000);
  } catch {
    throw new Error(`${n} cursors expected, ${await page.eval(count)} found; text ${JSON.stringify(await page.eval(editorText))}`);
  }
}

/** Escape closes a completion list if one is open, and collapses several cursors into one. */
async function collapse(page) {
  await page.key("Escape", "Escape", 27);
  await page.sleep(100);
  await page.key("Escape", "Escape", 27);
}

async function cursorsInDepth(page) {
  const cursors = `${visibleTab}.querySelectorAll('.cm-cursor').length`;
  // Cursors above and below: typing goes to all of them.
  await setEditor(page, "foo\nfoo\nfoo");
  await page.key("Home", "Home", 36, CTRL);
  await page.key("ArrowDown", "ArrowDown", 40, CTRL | ALT);
  await page.key("ArrowDown", "ArrowDown", 40, CTRL | ALT);
  await cursorsAre(page, 3);
  await page.type("x");
  await page.waitFor(`${editorText} === 'xfoo\\nxfoo\\nxfoo'`);
  // Escape leaves one.
  await collapse(page);
  await cursorsAre(page, 1);
  // Up from the last line.
  await page.key("End", "End", 35, CTRL);
  await page.key("ArrowUp", "ArrowUp", 38, CTRL | ALT);
  await cursorsAre(page, 2);
  await page.type("!");
  await page.waitFor(`${editorText} === 'xfoo\\nxfoo!\\nxfoo!'`);
  await collapse(page);

  // Ctrl+D: the word at the cursor, then the next occurrences; typing replaces them all.
  await setEditor(page, "ab cd ab cd ab");
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < 3; i++) await page.key("d", "KeyD", 68, CTRL);
  await cursorsAre(page, 3);
  await page.type("Z");
  await page.waitFor(`${editorText} === 'Z cd Z cd Z'`);
  await collapse(page);
  await cursorsAre(page, 1);
}

/** The four editor aids in more depth: linter, spell check, outline and multiple cursors. */
async function editorInDepth(page) {
  await newEditor(page, NAME);
  await lintInDepth(page);
  await newEditor(page, NAME);
  await spellInDepth(page);
  await outlineInDepth(page);
  await cursorsInDepth(page);
}
