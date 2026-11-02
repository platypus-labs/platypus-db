// After typing in the SQL editor one Escape is enough to let Tab leave it (docs/SHORTCUTS.md);
// with the completion list open, Escape only closes the list and Tab still indents or accepts.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { visibleTab } from "../qa-jrn.mjs";
import { editorText, sqliteConnection } from "../query.mjs";

export function setup({ dir }) {
  sqliteConnection(dir, { file: "s_fix_escape.db", id: "00000000-0000-4000-8000-0000000f5004", name: "S fix escape", sql: "CREATE TABLE zebra_stripes (id INTEGER);" });
}

const content = `${visibleTab}.querySelector('.cm-content')`;
const inEditor = `document.activeElement?.closest('.cm-content') != null`;

async function newEditor(page, before) {
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`document.querySelectorAll('.tab').length === ${before + 1} && ${content} != null`, 10_000);
  await page.waitFor(`document.activeElement === ${content}`, 5_000);
}

export default async function (page) {
  // The first editor needs a connection: open one on a double click.
  await page.waitFor(`document.querySelector('.tree-row') !== null`, 15_000);
  await page.eval(`[...document.querySelectorAll('.tree-row')].find(r => r.textContent.includes('S fix escape')).id = 'fix-esc'`);
  await page.clickSel("#fix-esc", { clickCount: 2 });
  await page.waitFor(`${content} != null`, 15_000);
  await page.waitFor(`document.activeElement === ${content}`, 5_000);

  // Typed text, no list: one Escape, then Tab leaves the editor and does not indent.
  await page.type("select 1");
  await page.waitFor(`${editorText} === 'select 1'`, 5_000);
  await page.key("Escape", "Escape", 27);
  await page.key("Tab", "Tab", 9);
  await page.waitFor(`!(${inEditor})`, 5_000);
  assert.equal(await page.eval(editorText), "select 1", "Tab typed into the editor");

  // Back in the editor, typing goes on as usual.
  await page.eval(`${content}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("Backspace", "Backspace", 8);

  // With the list open Escape closes the list only: Tab then stays in the editor.
  await page.type("select * from zebra");
  await page.waitFor(`document.querySelector('.cm-tooltip-autocomplete') !== null`, 10_000);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.cm-tooltip-autocomplete') === null`, 5_000);
  assert.equal(await page.eval(inEditor), true, "Escape with the list open left the editor");
  assert.equal(await page.eval(editorText), "select * from zebra");
  // A second Escape arms the editor and Tab leaves it.
  await page.key("Escape", "Escape", 27);
  await page.key("Tab", "Tab", 9);
  await page.waitFor(`!(${inEditor})`, 5_000);
  assert.equal(await page.eval(editorText), "select * from zebra");
}
