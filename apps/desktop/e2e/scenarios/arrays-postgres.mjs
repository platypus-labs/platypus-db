// Arrays and composite values in the data editor, against the dev PostgreSQL from
// docker/compose.yml: one row per element or field, NULL per element, saved as literals.
// Skipped when it is not running.
import assert from "node:assert/strict";
import { join } from "node:path";
import { ALT, CTRL } from "../cdp.mjs";

export { setup, skip } from "./dev-postgres.mjs";

const NAME = "E2E PG";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const listInputs = `[...document.querySelectorAll('.list-editor-row input:not([type=checkbox])')]`;
const listButton = (label) =>
  `[...document.querySelectorAll('.list-editor button')].find(b => b.textContent === ${JSON.stringify(label)})`;

/** Replaces the editor's text and runs it (`script`: all of it, else the statement). */
async function run(page, sql, script = false) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql.slice(-20))})`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  if (script) await page.key("x", "KeyX", 88, ALT);
  else await page.key("Enter", "Enter", 13, CTRL);
  const idle = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.textContent.includes('Cancel'))?.disabled`;
  await page.waitFor(`${idle} && ${status} !== '' && !${status}.includes('Executing')`, 15_000);
}

const SETUP = [
  "DROP TABLE IF EXISTS demo.e2e_arrays",
  "DROP TYPE IF EXISTS demo.e2e_pair",
  "CREATE TYPE demo.e2e_pair AS (label text, n int)",
  "CREATE TABLE demo.e2e_arrays (id int PRIMARY KEY, tags text[], pair demo.e2e_pair)",
  `INSERT INTO demo.e2e_arrays VALUES (1, '{a,"b c",NULL}', '(x,1)');`,
].join(";\n");

export default async function (page, { shots }) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`${row("demo")} !== undefined`, 15_000);

  await page.eval(`${row(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await run(page, SETUP, true);
  await run(page, "SELECT * FROM demo.e2e_arrays");
  assert.equal(await page.eval(status), "1 row");
  await page.waitFor(`${visibleTab}?.querySelector('.edit-bar:not([hidden]) button')?.disabled === false`, 15_000);

  // The array: one row per element, NULL ticked for the third.
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.key("F2", "F2", 113);
  await page.waitFor(`${listInputs}.length === 3`);
  assert.deepEqual(await page.eval(`${listInputs}.map(i => i.value)`), ["a", "b c", ""]);
  assert.deepEqual(
    await page.eval(`[...document.querySelectorAll('.list-editor-row input[type=checkbox]')].map(c => c.checked)`),
    [false, false, true],
  );
  await page.eval(`${listInputs}[0].focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("z");
  await page.eval(`${listButton("+ Element")}.click()`);
  await page.waitFor(`${listInputs}.length === 4`);
  await page.eval(`${listInputs}[3].focus()`);
  await page.type("new, one");
  await page.waitFor(`${listInputs}[3].value === 'new, one'`);
  if (shots) await page.screenshot(join(shots, "arrays-list-editor.png"));
  await page.eval(`${listButton("Apply")}.click()`);
  await page.waitFor(`document.querySelector('.list-editor') === null`);

  // The composite: one row per field, named.
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.key("F2", "F2", 113);
  await page.waitFor(`${listInputs}.length === 2`);
  assert.deepEqual(
    await page.eval(`[...document.querySelectorAll('.list-editor-label')].map(l => l.textContent)`),
    ["label", "n"],
  );
  await page.eval(`${listInputs}[1].focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("2");
  await page.waitFor(`${listInputs}[1].value === '2'`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.list-editor') === null`);

  await page.eval(`[...${visibleTab}.querySelectorAll('.edit-bar button')].find(b => b.textContent === 'Preview SQL').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  const sql = await page.eval(`document.querySelector('.sql-preview').textContent`);
  assert.match(sql, /tags = '\{z,"b c",NULL,"new, one"\}'/);
  assert.match(sql, /pair = '\(x,2\)'/);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  await page.waitFor(`${status} === '1 row'`, 15_000);

  // What the server stored, read back.
  await run(page, "SELECT tags::text || ' ' || pair::text AS v FROM demo.e2e_arrays");
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("c", "KeyC", 67, CTRL);
  await page.waitFor(`window.__copied !== undefined`);
  assert.equal(await page.eval(`window.__copied`), `{z,"b c",NULL,"new, one"} (x,2)`);

  await run(page, "DROP TABLE demo.e2e_arrays; DROP TYPE demo.e2e_pair;", true);
}
