// JRN-11 (low, keyboard). After a new connection is saved (Enter in the form), the focus is
// dropped on the page body: a keyboard user has to Tab through the whole title bar and the
// rail (about 15 stops) to reach the connection just made. The same happens after Ctrl+S
// saves the grid's edits: the focus leaves the grid. DBeaver selects the new connection in
// its navigator.
// Steps: first start; Tab to "New connection" in the navigator's header, Enter; type
// "sqlite", Tab, Enter; type a name, Tab to "Database file", type a path, Enter.
// Expected: the focus on the new connection's row in the navigator. Actual: on <body>.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx (closing on save restores the
// focus to nothing: the opener, the picker, is gone).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

export const ready = `document.querySelector('.sidebar-header') !== null`;
const focused = `(() => { const e = document.activeElement; return e === document.body ? 'body' : (e.closest('.tree-row')?.querySelector('.tree-label')?.textContent ?? e.tagName + ' ' + (e.getAttribute('aria-label') || e.textContent.trim().slice(0, 40))); })()`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "kb.db"));
  db.exec("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);");
  db.close();
}

/** A key that also types its character: Enter on a button presses it. */
async function press(page, key, code, keyCode, text) {
  await page.send("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode: keyCode, text, unmodifiedText: text });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: keyCode });
  await page.sleep(150);
}
const tab = (page) => press(page, "Tab", "Tab", 9, undefined);
const enter = (page) => press(page, "Enter", "Enter", 13, "\r");

export default async function (page, { dir }) {
  for (let i = 0; i < 20 && (await page.eval(`document.activeElement.getAttribute('aria-label') ?? ''`)) !== "New connection"; i++) await tab(page);
  await enter(page);
  await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`);
  await page.type("sqlite");
  await tab(page);
  await enter(page);
  await page.waitFor(`document.querySelector('.connection-dialog') !== null`);
  await page.type("Keyboard db");
  for (let i = 0; i < 12; i++) {
    if ((await page.eval(`document.activeElement.closest('label.field')?.querySelector('span')?.textContent ?? ''`)) === "Database file") break;
    await tab(page);
  }
  await page.type(join(dir, "kb.db"));
  await enter(page);
  await page.waitFor(`document.querySelector('.connection-dialog') === null`, 10_000);
  await page.sleep(500);
  assert.equal(await page.eval(focused), "Keyboard db");
}
