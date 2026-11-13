// QA journey (works, regression guard): a first day. First start with no data; a
// PostgreSQL connection made from a pasted URL (password kept in the system keychain);
// a table opened from the navigator, a cell edited and saved; a query written in a new
// editor, its result exported to CSV and the script saved to a file; the window closed and
// the app started again: the connection connects without asking, the tabs are back with the
// file's name and text. The connection is deleted at the end, its keychain entry with it.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { button as dialogButton, dialog, notice, paste, testConnection, newFromUrl } from "../conn-helpers.mjs";
import { needsPostgres, openPath, psql, psqlTry, restart, status, treeRow, visibleTab } from "../journey-helpers.mjs";
import { menu } from "../navigator-helpers.mjs";
import { answerSave, exportResult } from "../transfer-helpers.mjs";
import { clearStatus, editCell, focusCell, idle, save } from "../grid-helpers.mjs";

export const skip = needsPostgres;
export const ready = `document.querySelector('.sidebar-header') !== null`;
const URL = "postgresql://platypus:platypus@localhost:55432/platypus?sslmode=disable";
const SQL = "select name, price from qa_jrn_day order by id";
const tabTitles = `[...document.querySelectorAll('.tab')].map(t => t.textContent.replace(/×$/, ''))`;

export function setup() {
  psql(
    "DROP TABLE IF EXISTS qa_jrn_day; CREATE TABLE qa_jrn_day (id serial PRIMARY KEY, name text, price numeric(12,2)); " +
      "INSERT INTO qa_jrn_day (name, price) VALUES ('alpha', 1.50), ('beta', 2.25), ('gamma', NULL);",
  );
}

export default async function (page, ctx) {
  try {
    // Nothing yet: the welcome page offers the first connection.
    assert.equal(await page.eval(`document.querySelectorAll('.tree-row').length`), 0);
    await page.eval(`document.querySelector('.welcome-cta').click()`);
    await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`);
    await newFromUrl(page, URL);
    assert.match(await testConnection(page), /^Connected/, await page.eval(notice));
    await page.eval(`${dialogButton(dialog, "Save")}.click()`);
    await page.waitFor(`${dialog} === null`, 15_000);

    // Explore, open the table, edit a cell and save.
    await openPath(page, ["platypus", "public", "Tables"], "qa_jrn_day");
    await page.eval(`${treeRow("qa_jrn_day")}.id = 'jrn-t'`);
    await page.clickSel("#jrn-t", { clickCount: 2 });
    await page.waitFor(`${status} === '3 rows'`, 20_000);
    await focusCell(page, 0, 1);
    await editCell(page, "alpha2");
    assert.doesNotMatch(await save(page), /^ERROR/);
    assert.equal(psql("SELECT string_agg(name, ',' ORDER BY id) FROM qa_jrn_day"), "alpha2,beta,gamma");

    // A query of its own, exported and saved.
    await page.key("t", "KeyT", 84, CTRL);
    await page.waitFor(`document.querySelectorAll('.tab').length === 2 && ${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
    await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
    await page.type(SQL);
    await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === ${JSON.stringify(SQL)}`);
    await page.key("Escape", "Escape", 27);
    await clearStatus(page);
    await page.key("Enter", "Enter", 13, CTRL);
    assert.equal(await idle(page), "3 rows");
    const csv = join(ctx.dir, "day.csv");
    assert.match(await exportResult(page, csv), /^Exported 3 rows/);
    assert.equal(readFileSync(csv, "utf8").replace(/\r\n/g, "\n"), "name,price\nalpha2,1.50\nbeta,2.25\ngamma,\n");
    const file = join(ctx.dir, "day.sql");
    await answerSave(page, file);
    await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
    await page.key("s", "KeyS", 83, CTRL);
    await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('day.sql')`, 10_000);
    assert.equal(existsSync(file) && readFileSync(file, "utf8"), SQL);
    const titles = await page.eval(tabTitles);

    // Restart.
    await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('day.sql')`);
    await restart(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
    await page.waitFor(`document.querySelectorAll('.tab').length === 2`, 15_000);
    assert.deepEqual(await page.eval(tabTitles), titles);
    await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent === ${JSON.stringify(SQL)}`, 10_000);
    // The keychain answers: no password prompt.
    await page.eval(`${treeRow("platypus")}.querySelector('.twisty').click()`);
    await page.waitFor(`${treeRow("public")} !== undefined || document.querySelector('.modal input[type=password]') !== null`, 20_000);
    assert.equal(await page.eval(`document.querySelector('.modal input[type=password]')`), null, "the saved password was asked again");

    // Clean up: the connection and its keychain entry.
    await menu(page, "platypus", "Delete…");
    await page.waitFor(`document.querySelector('.modal') !== null`);
    await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Delete').click()`);
    await page.waitFor(`${treeRow("platypus")} === undefined`, 10_000);
  } finally {
    psqlTry("SET lock_timeout = '10s'; DROP TABLE IF EXISTS qa_jrn_day");
  }
}
