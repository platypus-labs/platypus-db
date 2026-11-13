// QA ui: in Italian the same keys are named two ways.
//
// Steps: language Italiano; open a SQL editor and hover the Script button; open the command
// palette; open a table's grid menu.
// Expected: one naming: the tooltips say "Ctrl+Maiusc+Invio", "(Ins)", "(Canc)", so the
// menus, the palette and the Settings should say "Ctrl+Maiusc+T", "Maiusc+Canc", "Ins", "Canc".
// Actual: tooltips are translated texts ("Esegui lo script (Alt+X, Ctrl+Maiusc+Invio)",
// "Aggiungi una riga (Ins)"), while every label built from an action's shortcut stays English
// ("Ctrl+Shift+T", "Shift+Delete", "Insert", "Delete", "Ctrl+Alt+Up").
// Code: actions.ts shortcutLabel() (English key names, no t()), versus the hand-written
// tooltips in components/QueryTab.tsx and the grid's edit bar.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow, visibleTab } from "../query.mjs";

const NAME = "QA UI tasti";

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_keys.db",
    id: "00000000-0000-4000-8000-00000000e125",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export default async function (page) {
  await page.waitFor(`document.documentElement.lang === 'it'`);
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.toolbar') != null`);
  const script = await page.eval(`[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => (b.title ?? '').startsWith('Esegui lo script')).title`);
  assert.match(script, /Ctrl\+Maiusc\+Invio/);

  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') === document.activeElement`);
  await page.type("riapri");
  const reopen = `[...document.querySelectorAll('.palette li')].find(l => l.querySelector('.palette-label')?.textContent === 'Riapri la scheda chiusa')`;
  await page.waitFor(`${reopen} !== undefined`);
  assert.equal(await page.eval(`${reopen}.querySelector('kbd')?.textContent`), "Ctrl+Maiusc+T");
}
