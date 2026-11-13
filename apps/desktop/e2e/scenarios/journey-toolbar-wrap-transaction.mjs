// JRN-09 (low). At the default window size (1280×800) the SQL editor's toolbar wraps onto a
// second row as soon as a transaction is open: the label "Transaction open 0:02 · 1 change"
// ("Transazione aperta 0:02 · 1 modifica" in Italian) pushes the last buttons (Format,
// Outline, History) to a row of their own, and the editor and its text jump down by a row
// while the user works. Seen in English and in Italian (QA_JRN_LANG=en runs it in English).
// Steps: SQL editor on a SQLite connection; run "BEGIN; INSERT …" (Alt+X).
// Expected: the toolbar keeps one row (the label shortened, or the buttons kept in place).
// Actual: it goes from 44 to 78 px high (screenshot it-toolbar-wrap.png).
// Suspected: apps/desktop/src/components/QueryTab.tsx toolbar (the transaction label does not
// shrink) and its CSS.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { ALT, CTRL } from "../cdp.mjs";
import { treeRow, visibleTab } from "../journey-helpers.mjs";
import { sqliteConnection } from "../query.mjs";

const NAME = "QA jrn toolbar";
const toolbar = `${visibleTab}.querySelector('.toolbar')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_jrn_toolbar.db",
    id: "00000000-0000-4000-8000-0000000a8003",
    name: NAME,
    sql: "CREATE TABLE logs (msg TEXT);",
  });
  writeFileSync(join(dir, "settings.toml"), `[appearance]\nlanguage = "${process.env.QA_JRN_LANG ?? "it"}"\n`);
}

export default async function (page, { shots }) {
  await page.eval(`${treeRow(NAME)}.id = 'jrn-c'`);
  await page.clickSel("#jrn-c", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  // The default size of the window.
  assert.deepEqual(await page.eval(`[innerWidth, innerHeight]`), [1280, 800]);
  const before = await page.eval(`${toolbar}.getBoundingClientRect().height`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("BEGIN;\nINSERT INTO logs VALUES ('x');");
  await page.key("Escape", "Escape", 27);
  await page.key("x", "KeyX", 88, ALT);
  // The transaction's label shows in the toolbar.
  await page.waitFor(`/0:0\\d/.test(${toolbar}.textContent)`, 10_000);
  await page.sleep(300);
  if (shots) await page.screenshot(join(shots, "it-toolbar-wrap.png"));
  const after = await page.eval(`${toolbar}.getBoundingClientRect().height`);
  // Roll back, so closing asks nothing.
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("ROLLBACK");
  await page.key("Escape", "Escape", 27);
  await page.key("Enter", "Enter", 13, CTRL);
  assert.equal(after, before, `the toolbar grew from ${before} to ${after} px with a transaction open`);
}
