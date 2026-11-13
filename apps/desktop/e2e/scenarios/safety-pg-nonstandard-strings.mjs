// BUG SAFE-04 (known, being fixed elsewhere): on PostgreSQL with standard_conforming_strings
// off, the data editor's save writes string literals as '...' with only quotes doubled, so
// backslashes in a value are read as escapes: the saved value is not the one typed.
// Steps: PG editor: SET standard_conforming_strings = off; SELECT * FROM qa_safe_scs; edit the
// v cell of row 1 to C:\temp\new; edit row 2 too (a save of several rows); Ctrl+S / Save.
// Expected: both rows hold exactly C:\temp\new (the app writes E'' literals, or parameters).
// Actual: \t and \n are turned into a tab and a newline (or the save fails).
// Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { connectionToml, dbExec, needs, openEditor, status, visibleTab } from "../safety-helpers.mjs";

const VALUE = "C:\\temp\\new";
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;

export const skip = needs("postgres");

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${connectionToml("postgres", "00000000-0000-4000-8000-000000000451", "QA PG")}`);
  dbExec("postgres", "DROP TABLE IF EXISTS qa_safe_scs; CREATE TABLE qa_safe_scs (id int primary key, v text); INSERT INTO qa_safe_scs VALUES (1, 'a'), (2, 'b')");
}

async function run(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${status} !== '' && !${status}.includes('Executing')`, 20_000);
}

async function editCell(page, text) {
  // F2 sent while the grid is still taking focus is lost: try again once.
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`, 3_000).catch(async () => {
    await page.key("F2", "F2", 113);
    await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  });
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
}

export default async function (page) {
  try {
    await openEditor(page, "QA PG");
    await run(page, "SET standard_conforming_strings = off");
    await run(page, "SELECT * FROM qa_safe_scs ORDER BY id");
    await page.waitFor(`${status} === '2 rows'`);
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
    await page.key("ArrowDown", "ArrowDown", 40);
    await page.key("ArrowRight", "ArrowRight", 39);
    await editCell(page, VALUE); // Enter moves to row 2
    await editCell(page, VALUE);
    await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
    await page.key("s", "KeyS", 83, CTRL);
    await page.waitFor(`${editBar}.includes('to edit a cell') && !${editBar}.includes('changed') || document.querySelector('.notice.error') !== null`, 15_000);
    await page.sleep(500);
    const stored = dbExec("postgres", "SELECT string_agg(v, '|' ORDER BY id) FROM qa_safe_scs");
    assert.equal(stored, `${VALUE}|${VALUE}`, `stored: ${JSON.stringify(stored)}`);
  } finally {
    dbExec("postgres", "DROP TABLE IF EXISTS qa_safe_scs");
  }
}

