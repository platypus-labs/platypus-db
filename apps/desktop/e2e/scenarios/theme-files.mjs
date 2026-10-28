// Theme files (Settings → Themes): Export saves the theme in use as JSON, both variants for
// System; Import reads a file, lists what it skips and previews it; Cancel goes back, Apply
// makes it the theme (kept in the settings, named in the top bar), Remove goes back to
// System; files that are not themes, or from a newer version, are refused with a message.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { answerSave } from "../qa-xfer.mjs";

const SEA = "#0b1d26";
const SAND = "#f5ecd7";
const bg = `getComputedStyle(document.documentElement).getPropertyValue('--bg').trim()`;
const preview = `document.querySelector('[data-theme-preview]')`;
const errorNotice = `document.querySelector('[data-notice="theme-error"]')?.textContent ?? ''`;
const click = (action) => `document.querySelector('[data-action=${JSON.stringify(action)}]').click()`;

export function setup({ dir }) {
  // The app opens on a profile with a connection.
  new DatabaseSync(join(dir, "t.db")).close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 4

[[connections]]
id = "00000000-0000-4000-8000-000000000041"
name = "Local"
driver = "sqlite"
database = '${join(dir, "t.db")}'
`,
  );
  writeFileSync(
    join(dir, "sea.json"),
    JSON.stringify({
      platypus_theme: 1,
      name: "Sea",
      dark: { bg: SEA, accent: "#3fc1c9", glow: "#ffffff", fg: "not-a-color" },
      author: "someone",
    }),
  );
  writeFileSync(join(dir, "sand.json"), JSON.stringify({ platypus_theme: 1, name: "Sand", light: { bg: SAND } }));
  writeFileSync(join(dir, "newer.json"), JSON.stringify({ platypus_theme: 9, dark: { bg: SEA } }));
  writeFileSync(join(dir, "broken.json"), "{ not json");
}

async function openThemes(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Themes");
  await page.waitFor(`document.querySelector('[data-action="import-theme"]') !== null`);
}

/** Imports `file` through the open dialog. */
async function importFile(page, dir, file) {
  await answerSave(page, join(dir, file), "plugin:dialog|open");
  await page.eval(click("import-theme"));
}

async function exportSystem(page, dir) {
  const path = join(dir, "exported.json");
  await answerSave(page, path);
  await page.eval(click("export-theme"));
  await page.waitFor(`document.querySelector('[data-notice="theme"]')?.textContent.includes('exported.json')`);
  const doc = JSON.parse(readFileSync(path, "utf8"));
  assert.equal(doc.platypus_theme, 1);
  assert.equal(doc.name, "System");
  assert.match(doc.light.bg, /^#/);
  assert.match(doc.dark.bg, /^#/);
  assert.notEqual(doc.light.bg, doc.dark.bg);
  assert.ok(doc.dark["syn-keyword"], "the editor's colors too");
}

async function refused(page, dir) {
  await importFile(page, dir, "broken.json");
  await page.waitFor(`(${errorNotice}).includes('not JSON')`);
  await importFile(page, dir, "newer.json");
  await page.waitFor(`(${errorNotice}).includes('newer PlatypusDB')`);
  assert.equal(await page.eval(`${preview} === null`), true);
}

async function previewThenCancel(page, dir, before) {
  await importFile(page, dir, "sea.json");
  await page.waitFor(`${preview}?.dataset.themePreview === 'Sea'`);
  assert.equal(await page.eval(errorNotice), "", "the error goes once a file reads");
  const skipped = await page.eval(`[...document.querySelectorAll('.theme-skipped code')].map(c => c.textContent)`);
  assert.deepEqual(skipped.sort(), ["author: unknown", "dark.fg: not a color", "dark.glow: unknown"]);
  await page.waitFor(`${bg} === '${SEA}'`);
  assert.equal(await page.eval(`document.documentElement.dataset.theme`), "dark");
  assert.equal(await page.eval(`document.querySelector('[data-action="export-theme"]').disabled`), true);
  await page.eval(click("cancel-theme"));
  await page.waitFor(`${bg} === '${before}'`);
  assert.equal(await page.eval(`${preview} === null`), true);
}

async function applyAndRemove(page, dir, shots) {
  await importFile(page, dir, "sea.json");
  await page.waitFor(`${preview} !== null`);
  if (shots) await page.screenshot(join(shots, "theme-preview.png"));
  await page.eval(click("apply-theme"));
  await page.waitFor(`document.querySelector('[data-imported-theme="Sea"]')?.textContent.includes('in use')`);
  assert.equal(await page.eval(bg), SEA);
  const settings = readFileSync(join(dir, "settings.toml"), "utf8");
  assert.match(settings, /theme = "custom"/);
  assert.match(settings, /custom_theme = .*Sea/);
  // The top bar's theme button names it.
  assert.ok(await page.eval(`[...document.querySelectorAll('button[title]')].some(b => b.title.startsWith('Theme: Sea'))`));

  // The exported file of an imported theme is that theme, dark only.
  const path = join(dir, "sea-again.json");
  await answerSave(page, path);
  await page.eval(click("export-theme"));
  await page.waitFor(`document.querySelector('[data-notice="theme"]')?.textContent.includes('sea-again.json')`);
  const doc = JSON.parse(readFileSync(path, "utf8"));
  assert.equal(doc.name, "Sea");
  assert.equal(doc.dark.bg, SEA);
  assert.equal(doc.light, undefined);

  // A light-only theme replaces it.
  await importFile(page, dir, "sand.json");
  await page.waitFor(`${preview}?.dataset.themePreview === 'Sand'`);
  await page.eval(click("apply-theme"));
  await page.waitFor(`document.querySelector('[data-imported-theme="Sand"]') !== null`);
  assert.equal(await page.eval(bg), SAND);
  assert.equal(await page.eval(`document.documentElement.dataset.theme`), "light");

  await page.eval(click("remove-theme"));
  await page.waitFor(`document.querySelector('[data-imported-theme]') === null`);
  await page.waitFor(`${bg} !== '${SAND}'`);
  assert.doesNotMatch(readFileSync(join(dir, "settings.toml"), "utf8"), /theme = "custom"/);
}

export default async function (page, { dir, shots }) {
  await openThemes(page);
  const before = await page.eval(bg);
  await exportSystem(page, dir);
  await refused(page, dir);
  await previewThenCancel(page, dir, before);
  await applyAndRemove(page, dir, shots);
}
