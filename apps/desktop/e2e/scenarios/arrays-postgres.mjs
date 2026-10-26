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
  const idle = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')?.disabled`;
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

  // A new element's field takes the focus; removing an element with the mouse keeps the
  // editor open (the removed button had the focus, and its loss is no click elsewhere).
  await page.eval(`${listButton("+ Element")}.click()`);
  await page.waitFor(`${listInputs}.length === 5 && document.activeElement === ${listInputs}[4]`);
  await page.type("gone");
  await page.waitFor(`${listInputs}[4].value === 'gone'`);
  await page.clickSel(".list-editor-row:nth-child(5) button.icon");
  await page.waitFor(`${listInputs}.length === 4`);
  assert.equal(await page.eval(`document.querySelector('.list-editor')?.contains(document.activeElement) === true`), true);

  // The layout at the popup's least size (a small grid): the actions wrap but stay inside,
  // the primary one last, and the elements scroll in the body.
  const fits = `(() => {
    const popup = document.querySelector('.list-editor').getBoundingClientRect();
    const buttons = [...document.querySelectorAll('.list-editor .cell-popup-actions button')];
    return buttons.length > 0 && buttons.every((b) => {
      const r = b.getBoundingClientRect();
      return r.left >= popup.left && r.right <= popup.right && r.top >= popup.top && r.bottom <= popup.bottom;
    });
  })()`;
  assert.equal(await page.eval(fits), true);
  assert.equal(
    await page.eval(`[...document.querySelectorAll('.list-editor .cell-popup-actions button')].at(-1).textContent`),
    "Apply",
  );
  const size = await page.eval(
    `(() => { const s = document.querySelector('.list-editor').style; const old = [s.width, s.height]; s.width = '280px'; s.height = '160px'; return old; })()`,
  );
  assert.equal(await page.eval(fits), true);
  assert.equal(
    await page.eval(`(() => { const r = document.querySelector('.list-editor-rows'); return r.scrollHeight > r.clientHeight; })()`),
    true,
  );
  await page.eval(`(() => { const s = document.querySelector('.list-editor').style; [s.width, s.height] = ${JSON.stringify(size)}; })()`);
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

  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Preview SQL"]').click()`);
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
  const stored = `{z,"b c",NULL,"new, one"} (x,2)`;
  assert.equal(await copyGrid(page, stored), stored);

  // Arrays shown as PostgreSQL prints them: bounds other than 1 first, `;` between boxes.
  await run(page, "SELECT '[0:1]={1,2}'::int[] AS lb, ARRAY['(1,1),(0,0)'::box, '(2,2),(1,1)'::box] AS bx");
  const shown = "[0:1]={1,2}\t{(1,1),(0,0);(2,2),(1,1)}";
  assert.equal(await copyGrid(page, shown), shown);

  // An array with bounds is edited as text, bounds included, so that saving keeps them.
  await run(page, "UPDATE demo.e2e_arrays SET tags = '[0:1]={x,y}' WHERE id = 1");
  await run(page, "SELECT * FROM demo.e2e_arrays");
  await page.waitFor(`${visibleTab}?.querySelector('.edit-bar:not([hidden]) button')?.disabled === false`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.list-editor textarea')?.value === '[0:1]={x,y}'`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.list-editor') === null`);

  await run(page, "DROP TABLE demo.e2e_arrays; DROP TYPE demo.e2e_pair;", true);
}

/** Copies the whole grid of the visible tab. The status ends before the canvas grid has the
 *  rows' values: a copy of an empty grid gives "" or writes nothing, and the grid may still
 *  hold the previous result. Copy again, each attempt bounded, until the copy is `expected`
 *  or time is up, and return the last copy. */
async function copyGrid(page, expected) {
  let copied = "";
  const deadline = Date.now() + 15_000;
  while (copied !== expected && Date.now() < deadline) {
    await page.eval(`window.__copied = undefined; ${visibleTab}.querySelector('.grid-scroll').focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    await page.key("c", "KeyC", 67, CTRL);
    try {
      await page.waitFor(`window.__copied !== undefined`, 500);
    } catch {
      // Nothing was copied: try again.
    }
    copied = (await page.eval(`window.__copied`)) ?? "";
    if (copied !== expected) await new Promise((r) => setTimeout(r, 100));
  }
  return copied;
}
