// In Italian the open-transaction button ("Transazione aperta 0:00 · 1 modifica") must show whole,
// not cut with an ellipsis, with the toolbar still on one row (QA_JRN_LANG=en runs it in English).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { ALT, CTRL } from "../cdp.mjs";
import { treeRow, visibleTab } from "../journey-helpers.mjs";
import { sqliteConnection } from "../query.mjs";

const NAME = "S fix tx label";
const toolbar = `${visibleTab}.querySelector('.toolbar')`;

export function setup({ dir }) {
  sqliteConnection(dir, { file: "s_fix_tx.db", id: "00000000-0000-4000-8000-0000000f5002", name: NAME, sql: "CREATE TABLE logs (msg TEXT);" });
  writeFileSync(join(dir, "settings.toml"), `[appearance]\nlanguage = "${process.env.QA_JRN_LANG ?? "it"}"\n`);
}

export default async function (page) {
  await page.eval(`${treeRow(NAME)}.id = 'fix-c'`);
  await page.clickSel("#fix-c", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  const before = await page.eval(`${toolbar}.getBoundingClientRect().height`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("BEGIN;\nINSERT INTO logs VALUES ('x');");
  await page.key("Escape", "Escape", 27);
  await page.key("x", "KeyX", 88, ALT);
  await page.waitFor(`${toolbar}.querySelector('button.tx-open')?.textContent.match(/ 1 (change|modific)/)`, 10_000);
  const fit = `(() => { const b = ${toolbar}.querySelector('button.tx-open'); return [b.scrollWidth, b.clientWidth, b.textContent]; })()`;
  const [scroll, client, text] = await page.eval(fit);
  assert.ok(scroll <= client, `the transaction label is cut: "${text}" needs ${scroll} px, has ${client}`);
  assert.equal(await page.eval(`${toolbar}.getBoundingClientRect().height`), before, "the toolbar wrapped");
  // Two changes: the plural form fits as well.
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("INSERT INTO logs VALUES ('y'), ('z');");
  await page.key("Escape", "Escape", 27);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${toolbar}.querySelector('button.tx-open')?.textContent.match(/ 2 (change|modific)/)`, 10_000);
  const [scroll2, client2, text2] = await page.eval(fit);
  assert.ok(scroll2 <= client2, `the label with 2 changes is cut: "${text2}" needs ${scroll2} px, has ${client2}`);
  assert.equal(await page.eval(`${toolbar}.getBoundingClientRect().height`), before, "the toolbar wrapped");
  // Roll back, so closing asks nothing.
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("ROLLBACK");
  await page.key("Escape", "Escape", 27);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${toolbar}.querySelector('button.tx-open') === null`, 10_000);
}
