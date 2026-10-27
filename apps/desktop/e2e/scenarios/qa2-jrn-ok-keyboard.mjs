// QA journey (works, regression guard): a whole first task with the keyboard only. Tab to
// "New connection", Enter; type "sqlite" in the picker, Tab, Enter; type the name, Tab to the
// file, type it, Enter saves; Tab into the navigator, the arrows open the connection, its
// schema and its tables, Enter opens a table's data; from the SQL editor Escape then Tab
// leaves it (Tab alone indents) and reaches the grid; arrows, F2, typing and Enter edit a
// cell; Ctrl+S saves it. (Ctrl+T then typing is JRN-14: the new editor gets no focus.)
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { status, visibleTab } from "../qa-jrn.mjs";

export const ready = `document.querySelector('.sidebar-header') !== null`;
const focusLabel = `(document.activeElement.getAttribute('aria-label') ?? '')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "kb.db"));
  db.exec("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT); INSERT INTO notes VALUES (1, 'uno'), (2, 'due');");
  db.close();
}

/** A key as the keyboard sends it: Enter also types its character, so it presses buttons. */
async function press(page, key, code, keyCode, modifiers = 0) {
  const text = key === "Enter" && !modifiers ? "\r" : undefined;
  await page.send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", key, code, windowsVirtualKeyCode: keyCode, modifiers, text, unmodifiedText: text });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: keyCode, modifiers });
  await page.sleep(120);
}
const tab = (page) => press(page, "Tab", "Tab", 9);
const enter = (page) => press(page, "Enter", "Enter", 13);
const down = (page) => press(page, "ArrowDown", "ArrowDown", 40);
const right = (page) => press(page, "ArrowRight", "ArrowRight", 39);

async function tabUntil(page, test, max = 30) {
  for (let i = 0; i < max; i++) {
    if (await page.eval(test)) return;
    await tab(page);
  }
  throw new Error(`Tab never reached ${test}`);
}

export default async function (page, { dir }) {
  await tabUntil(page, `${focusLabel} === 'New connection'`);
  await enter(page);
  await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`);
  await page.type("sqlite");
  await tab(page);
  assert.equal(await page.eval(`document.activeElement.textContent`), "SQLite");
  await enter(page);
  await page.waitFor(`document.querySelector('.connection-dialog') !== null`);
  await page.type("Keyboard db");
  await tabUntil(page, `document.activeElement.closest('label.field')?.querySelector('span')?.textContent === 'Database file'`, 12);
  await page.type(join(dir, "kb.db"));
  await enter(page);
  await page.waitFor(`document.querySelector('.connection-dialog') === null`, 10_000);

  // The navigator: into the table's data.
  await tabUntil(page, `document.activeElement.classList.contains('tree-row')`);
  await right(page);
  await page.waitFor(`document.activeElement.getAttribute('aria-expanded') === 'true'`, 10_000);
  // Down the rows, opening the closed ones (the schema may open by itself), to the table.
  const focusedRow = `document.activeElement.querySelector('.tree-label')?.textContent`;
  for (let i = 0; i < 8 && (await page.eval(focusedRow)) !== "notes"; i++) {
    await down(page);
    await page.waitFor(`${focusedRow} !== undefined`);
    if (await page.eval(`document.activeElement.getAttribute('aria-expanded') === 'false'`)) {
      await right(page);
      await page.waitFor(`document.activeElement.getAttribute('aria-expanded') === 'true'`, 10_000);
    }
  }
  assert.equal(await page.eval(focusedRow), "notes");
  await enter(page);
  await page.waitFor(`${status} === '2 rows'`, 15_000);

  // From the editor to the grid, and an edit saved.
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await press(page, "Escape", "Escape", 27);
  await tabUntil(page, `document.activeElement.classList.contains('grid-scroll')`, 10);
  await down(page);
  await right(page);
  await press(page, "F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.type("tre");
  await enter(page);
  await press(page, "s", "KeyS", 83, CTRL);
  const read = () => {
    const db = new DatabaseSync(join(dir, "kb.db"));
    try {
      return db.prepare("SELECT body FROM notes WHERE id = 1").get().body;
    } finally {
      db.close();
    }
  };
  const deadline = Date.now() + 10_000;
  while (read() !== "tre" && Date.now() < deadline) await page.sleep(200);
  assert.equal(read(), "tre");
}
