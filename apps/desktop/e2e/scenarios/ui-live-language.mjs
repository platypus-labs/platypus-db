// QA ui: after switching the language, the open data viewers keep English placeholders.
//
// Steps: in English open a table (data viewer); Settings > Appearance > Language: Italiano.
// Expected: the whole window in Italian at once, as the Settings promise ("A new language
// shows at once", App.tsx).
// Actual: the status bar and the edit bar change, but the "SQL on the result" field keeps
// "SQL on the loaded rows: WHERE …, ORDER BY …, GROUP BY … or SELECT … FROM result" until the
// tab is opened again (it reads "SQL sulle righe caricate: ..." when the app starts in
// Italian). Screenshot: 27-light-it.png.
// Code: editor/SqlLine.tsx builds its CodeMirror state once, with
// `placeholder(props.placeholder)`, and never reconfigures it when the prop changes.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow, visibleTab } from "../query.mjs";

const NAME = "QA UI language";
const pick = (label, value) => `(() => {
  const row = [...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)});
  const el = row.querySelector('select');
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;
const placeholder = `(${visibleTab}?.querySelector('.result-sql-field .cm-placeholder')?.textContent ?? '')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_lang.db",
    id: "00000000-0000-4000-8000-00000000e127",
    name: NAME,
    sql: "CREATE TABLE qa_ui_orders (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO qa_ui_orders VALUES (1, 'a');",
  });
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("qa_ui_orders")} !== undefined`, 15_000);
  await page.eval(`${treeRow("qa_ui_orders")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${placeholder}.startsWith('Filter these rows')`, 15_000);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance", "Language and text");
  await page.eval(pick("Language", "it"));
  await page.waitFor(`document.querySelector('.modal.settings h2')?.textContent === 'Impostazioni'`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Fatto').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
  await page.waitFor(`(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '').includes('Doppio clic')`);
  assert.match(await page.eval(placeholder), /^Filtra queste righe/);
}
