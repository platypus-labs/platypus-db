// The README's GIFs on the throwaway SQLite shop of readme-sqlite: a query written with JOIN
// completion, run and drawn as a chart; then rows edited in the grid, their SQL previewed and
// saved. Runs only with PLATYPUS_README_GIFS set to the folder they go to (docs/screenshots):
//
//   PLATYPUS_README_GIFS=../../docs/screenshots npm run e2e -- gifs
import { join, resolve } from "node:path";
import { CTRL } from "../cdp.mjs";
import { pointAndClick, Recorder, typeSlowly } from "../gif.mjs";
import {
  closeTab,
  editorText,
  grid,
  NAME,
  openProject,
  openTable,
  prepare,
  setup as shop,
  status,
  treeRow,
  visibleTab,
} from "./readme-sqlite.mjs";

export function skip() {
  return process.env.PLATYPUS_README_GIFS ? null : "PLATYPUS_README_GIFS not set";
}

export const setup = shop;

const mark = (expr, id) => `(() => { const e = ${expr}; if (e) e.id = ${JSON.stringify(id)}; return !!e; })()`;

/** A query typed with the JOIN its foreign key completes, run, then shown as bars. */
async function editorGif(page, out) {
  await page.eval(mark(treeRow(NAME), "e2e-connection"));
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-connection')?.removeAttribute('id')`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 10_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("Delete", "Delete", 46);

  const gif = new Recorder(page);
  await gif.start();
  await gif.hold(600);
  await typeSlowly(page, "SELECT c.city, sum(o.total) AS revenue\nFROM orders o\nJOIN ");
  await page.key(" ", "Space", 32, CTRL);
  await page.waitFor(`[...document.querySelectorAll('.cm-tooltip-autocomplete li')].some(li => li.textContent.includes('customers c ON'))`, 10_000);
  await gif.hold(1200);
  // The entry with the condition from the foreign key.
  const entry = `[...document.querySelectorAll('.cm-tooltip-autocomplete li')].findIndex(li => li.textContent.includes('customers c ON'))`;
  const down = await page.eval(entry);
  for (let i = 0; i < down; i++) {
    await page.key("ArrowDown", "ArrowDown", 40);
    await page.sleep(150);
  }
  await page.sleep(400);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${editorText}.includes('customers c ON')`);
  await gif.hold(800);
  await typeSlowly(page, "\nGROUP BY c.city\nORDER BY revenue DESC");
  await page.key("Escape", "Escape", 27);
  await gif.hold(400);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`/^\\d+ rows$/.test(${status})`, 15_000);
  await page.eval(`document.activeElement?.blur()`);
  await gif.hold(1500);
  const chart = `[...${visibleTab}.querySelectorAll('.view-toggle button')].find(b => b.textContent === 'Chart')`;
  await page.eval(mark(chart, "e2e-chart"));
  await pointAndClick(page, "#e2e-chart");
  await page.waitFor(`${visibleTab}.querySelectorAll('.chart-svg rect.chart-bar').length > 0`, 10_000);
  await gif.hold(2000);
  const size = await gif.save(join(out, "editor.gif"));
  console.log(`editor.gif: ${(size / 1024).toFixed(0)} KB`);
  await closeTab(page);
}

/** Cells changed in the grid, a row added, the SQL previewed, then saved at once. */
async function editGif(page, out) {
  await openTable(page, "customers");
  await page.eval(`${grid}.focus()`);
  const gif = new Recorder(page);
  await gif.start();
  await gif.hold(800);
  for (const [key, code, n] of [["ArrowDown", "ArrowDown", 40], ["ArrowDown", "ArrowDown", 40], ["ArrowRight", "ArrowRight", 39], ["ArrowRight", "ArrowRight", 39]]) {
    await page.key(key, code, n);
    await page.sleep(200);
  }
  // A city changed by typing over it.
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.key("a", "KeyA", 65, CTRL);
  await typeSlowly(page, "Asti", 90);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
  await gif.hold(700);
  // A new row.
  const add = `${visibleTab}.querySelector('.edit-bar button[aria-label="Add a row"]')`;
  await page.eval(mark(add, "e2e-add"));
  await pointAndClick(page, "#e2e-add");
  await page.sleep(300);
  await page.eval(`${grid}.focus()`);
  await page.key("ArrowLeft", "ArrowLeft", 37);
  for (const text of ["Irene Villa", "Pisa"]) {
    await page.key("F2", "F2", 113);
    await page.waitFor(`document.querySelector('.cell-editor') !== null`);
    await typeSlowly(page, text, 70);
    await page.key("Tab", "Tab", 9);
    await page.waitFor(`document.querySelector('.cell-editor') === null`);
    await page.sleep(300);
  }
  await page.waitFor(`${visibleTab}.querySelector('.edit-bar').textContent.includes('2 rows changed')`);
  await gif.hold(800);
  const preview = `${visibleTab}.querySelector('.edit-bar button[aria-label="Preview SQL"]')`;
  await page.eval(mark(preview, "e2e-preview"));
  await pointAndClick(page, "#e2e-preview");
  await page.waitFor(`document.querySelector('.modal') !== null`);
  await gif.hold(2500);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal') === null`);
  const save = `${visibleTab}.querySelector('.edit-bar button.primary')`;
  await page.eval(mark(save, "e2e-save"));
  await pointAndClick(page, "#e2e-save");
  await page.waitFor(`!${visibleTab}.querySelector('.edit-bar').textContent.includes('changed')`, 10_000);
  await gif.hold(1500);
  const size = await gif.save(join(out, "data-editor.gif"));
  console.log(`data-editor.gif: ${(size / 1024).toFixed(0)} KB`);
  await closeTab(page);
}

export default async function (page) {
  const out = resolve(process.env.PLATYPUS_README_GIFS);
  await prepare(page);
  await openProject(page, 2, "Shop");
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("orders")} !== undefined`, 15_000);
  await editorGif(page, out);
  await editGif(page, out);
}
