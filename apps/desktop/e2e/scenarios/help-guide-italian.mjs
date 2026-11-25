// The guide in the app's language: with the interface in Italian the Help dialog shows the Italian
// pages (docs/user/translations/it/guide/), the index lists their titles, a link between two
// translated pages stays Italian, a page that is not translated (FEATURES.md) is shown in
// English with the note that the guide is in English, and after Settings > Language: English the
// guide is English again. It reads the real guide that ships, not the scenario's own pages.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { CTRL } from "../cdp.mjs";
import { seedSqlite } from "../conn-helpers.mjs";
import { italian } from "../journey-helpers.mjs";

const IT = italian();
const repo = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
/** The text of the first `# heading` of a Markdown file of the repository. */
const titleOf = (path) => /^# (.+)$/m.exec(readFileSync(join(repo, path), "utf8"))[1].trim();

const dialog = `document.querySelector('.guide-dialog:not(.whats-new)')`;
const reader = `${dialog}.querySelector('.guide-reader')`;
const heading = `(${reader}.querySelector('h1')?.textContent ?? '')`;
const pageOf = `${reader}.dataset.page`;
const note = `${dialog}.querySelector('.guide-language')`;
const indexTitle = (key) => `(${dialog}.querySelector('.guide-index button[data-page="${key}"]')?.textContent ?? '')`;
const pick = (label, value) => `(() => {
  const row = [...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)});
  const el = row.querySelector('select');
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;

export function setup({ dir }) {
  seedSqlite(dir);
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export const ready = `document.querySelector('.sidebar-header') !== null && window.__platypus !== undefined`;

async function openGuide(page, label) {
  await page.eval(`document.querySelector('.help-button').click()`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim() === ${JSON.stringify(label)}).click()`);
  await page.waitFor(`${dialog} !== null`);
}

async function closeGuide(page) {
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${dialog} === null`);
}

export default async function (page) {
  const itStart = titleOf("docs/user/translations/it/guide/getting-started.md");
  const enStart = titleOf("docs/user/guide/getting-started.md");
  const enFeatures = titleOf("docs/user/FEATURES.md");
  assert.notEqual(itStart, enStart);

  // Italian: the index and the first page are the translations, once they are loaded.
  await openGuide(page, IT["User guide"]);
  await page.waitFor(`${heading} === ${JSON.stringify(titleOf("docs/user/translations/it/guide/README.md"))}`, 15_000);
  await page.eval(`${dialog}.querySelector('.guide-index button[data-page="getting-started"]').click()`);
  await page.waitFor(`${pageOf} === 'getting-started' && ${heading} === ${JSON.stringify(itStart)}`);
  assert.equal(await page.eval(indexTitle("getting-started")), itStart, "the index shows the translated title");
  assert.equal(await page.eval(`${note}`), null, "no note on a translated page");
  assert.equal(await page.eval(`${reader}.querySelectorAll('img').length`), 1, "the English screenshot is shown");

  // A link between two translated pages stays in Italian.
  await page.eval(`[...${reader}.querySelectorAll('a')].find(a => a.dataset.href === 'connect.md').click()`);
  await page.waitFor(`${pageOf} === 'connect' && ${heading} === ${JSON.stringify(titleOf("docs/user/translations/it/guide/connect.md"))}`);

  // A page with no translation falls back to English, and the dialog says so.
  await page.eval(`${dialog}.querySelector('.guide-index button[data-page="getting-started"]').click()`);
  await page.waitFor(`${pageOf} === 'getting-started'`);
  await page.eval(`[...${reader}.querySelectorAll('a')].find(a => a.textContent === 'FEATURES.md').click()`);
  await page.waitFor(`${pageOf} === 'docs/user/FEATURES.md' && ${heading} === ${JSON.stringify(enFeatures)}`);
  assert.equal(await page.eval(`${note}.textContent`), IT["The guide is written in English."]);
  await closeGuide(page);

  // Back to English in Settings: the guide opens in English from then on.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance", "Language and text");
  await page.eval(pick(IT["Language"], "en"));
  await page.waitFor(`document.querySelector('.modal.settings h2')?.textContent === 'Settings'`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
  await openGuide(page, "User guide");
  await page.eval(`${dialog}.querySelector('.guide-index button[data-page="getting-started"]').click()`);
  await page.waitFor(`${pageOf} === 'getting-started' && ${heading} === ${JSON.stringify(enStart)}`);
  await page.sleep(500);
  assert.equal(await page.eval(heading), enStart, "still English after the translations had time to load");
  assert.equal(await page.eval(indexTitle("getting-started")), enStart);
  assert.equal(await page.eval(`${note}`), null, "no note in English");
  await closeGuide(page);
}
