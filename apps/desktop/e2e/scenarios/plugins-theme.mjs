// A theme as a plugin (examples/plugins/theme-sea): a package with no module, reviewed like any other,
// which shows up in the theme picker. Choosing it paints it and survives a restart of the page; a
// value that tries to leave its declaration, or names a resource, is refused at the review; a theme
// with poor contrast installs with a warning; its font comes from the package; turned off or removed,
// the theme falls back to the default, and a start with the plugin's files gone still starts.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { CTRL } from "../cdp.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(here, "../../../..");
const SEA_DARK = "#0b1d26";
const FONT = join(REPO, "apps/desktop/src/assets/fonts/inter-latin-ext-wght-normal.woff2");

const panel = `document.querySelector('.plugins-panel')`;
const review = `${panel}?.querySelector('[data-field="plugin-review"]')`;
const pluginRow = (id) => `${panel}?.querySelector('.driver-block[data-plugin="${id}"]:not(.plugin-review)')`;
const errorNotice = `(${panel}?.querySelector('[data-field="plugin-error"]')?.textContent ?? '')`;
const bg = `getComputedStyle(document.documentElement).getPropertyValue('--bg').trim()`;
const card = (label) => `[...document.querySelectorAll('.theme-card')].find(c => c.textContent === ${JSON.stringify(label)})`;
const cards = `[...document.querySelectorAll('.theme-card')].map(c => c.textContent)`;

const manifest = (id, theme) => `id = "${id}"
name = "${id}"
version = "1.0.0"
api = 2
kinds = ["theme"]

[theme]
${theme}
`;

function writePackage(dir, id, theme, files = {}) {
  const root = join(dir, "packages", id);
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, "plugin.toml"), manifest(id, theme));
  for (const [name, source] of Object.entries(files)) {
    mkdirSync(dirname(join(root, name)), { recursive: true });
    cpSync(source, join(root, name));
  }
}

export function setup({ dir }) {
  cpSync(join(REPO, "examples/plugins/theme-sea"), join(dir, "packages", "theme-sea"), { recursive: true });
  writePackage(dir, "escape", `name = "Escape"\ndark = { bg = "#000000; background: url(https://example.com/x.png)" }`);
  writePackage(dir, "remote", `name = "Remote"\ndark = { bg = "url(https://example.com/x.png)" }`);
  writePackage(dir, "script", `name = "Script"\ndark = { bg = "expression(alert(1))" }`);
  writePackage(dir, "closes", `name = "Closes"\ndark = { bg = "#000}body{display:none" }`);
  writePackage(dir, "unknown", `name = "Unknown"\ndark = { glow = "#000000" }`);
  writePackage(dir, "faint", `name = "Faint"\ndark = { bg = "#101010", fg = "#1c1c1c" }`);
  writePackage(dir, "webfont", `name = "Web font"\nfonts = { mono = "https://example.com/x.woff2" }\ndark = { bg = "#101010" }`);
  writePackage(dir, "typed", `name = "Typed"\nfonts = { mono = "fonts/Typed.woff2" }\ndark = { bg = "#102030" }`, { "fonts/Typed.woff2": FONT });
  writeFileSync(join(dir, "settings.toml"), `version = 1\n\n[drivers]\nauto_update = false\n`);
}

/** Waits for something outside the page. */
async function until(done) {
  const deadline = Date.now() + 10_000;
  while (!done()) {
    assert.ok(Date.now() < deadline, String(done));
    await new Promise((r) => setTimeout(r, 100));
  }
}

async function openSettings(page, section) {
  if ((await page.eval(`document.querySelector('.modal.settings') === null`)) === true) {
    await page.key(",", "Comma", 188, CTRL);
  }
  await page.settingsTab(section);
}

async function closeSettings(page) {
  await page.eval(`document.querySelector('.modal.settings [role="tab"][aria-selected="true"]')?.focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

/** Asks to install the package `id`; the caller waits for what comes of it. */
async function pick(page, dir, id) {
  await openSettings(page, "Plugins");
  await page.waitFor(`${panel} !== null`, 15_000);
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(join(dir, "packages", id))})`);
  await page.eval(`${panel}.querySelector('[data-action="install-folder"]').click()`);
}

async function openReview(page, dir, id) {
  await pick(page, dir, id);
  await page.waitFor(`${review}?.dataset.plugin === ${JSON.stringify(id)}`, 15_000);
}

async function install(page, dir, id) {
  await openReview(page, dir, id);
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow(id)} != null && ${review} === null`, 15_000);
}

async function refused(page, dir) {
  // Each refusal names its own value: the notice of the one before is not taken for it.
  for (const [id, why] of [
    ["escape", "background: url"],
    ["remote", "url(https://example.com/x.png)"],
    ["script", "expression(alert(1))"],
    ["closes", "display:none"],
    ["unknown", "dark.glow"],
    ["webfont", "example.com/x.woff2"],
  ]) {
    await pick(page, dir, id);
    await page.waitFor(`${errorNotice}.includes(${JSON.stringify(why)})`, 15_000);
    assert.equal(await page.eval(`${review} === null`), true, id);
    assert.equal(await page.eval(`${pluginRow(id)} == null`), true, id);
  }
  // Nothing of them reached the page.
  assert.notEqual(await page.eval(bg), "");
  assert.equal(await page.eval(`document.body.style.display`), "");
}

async function reviewSaysATheme(page, dir) {
  await openReview(page, dir, "theme-sea");
  const text = await page.eval(`${review}.textContent`);
  assert.match(text, /Theme “Sea”/);
  assert.match(text, /No permissions/);
  assert.equal(await page.eval(`${review}.querySelector('[data-field="theme-contrast"]') === null`), true, "it is legible");
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow("theme-sea")} != null && ${review} === null`, 15_000);
}

async function choosePaintsIt(page, dir) {
  await openSettings(page, "Appearance");
  await page.waitFor(`${card("Sea")} !== undefined`);
  assert.ok((await page.eval(cards)).includes("Nord"), "the app's own stay");
  await page.eval(`${card("Sea")}.click()`);
  await page.waitFor(`${card("Sea")}.getAttribute('aria-checked') === 'true'`);
  // The example has both variants: the page's mode picks one, and the setting names the plugin.
  await page.waitFor(`document.documentElement.dataset.theme === 'dark' ? ${bg} === '${SEA_DARK}' : ${bg} === '#f4fbfc'`);
  await until(() => /theme = "plugin:theme-sea"/.test(readFileSync(join(dir, "settings.toml"), "utf8")));
  // Set through a variable on <html>: no style element holds it.
  const inStyles = await page.eval(`[...document.querySelectorAll('style')].some(s => s.textContent.includes('${SEA_DARK}'))`);
  assert.equal(inStyles, false);
  await closeSettings(page);
}

async function warnsOnPoorContrast(page, dir) {
  await openReview(page, dir, "faint");
  const warning = await page.eval(`${review}.querySelector('[data-field="theme-contrast"]')?.textContent ?? ''`);
  assert.match(warning, /hard to read/);
  assert.match(warning, /fg \/ bg/);
  // A warning: it still installs.
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow("faint")} != null && ${review} === null`, 15_000);
  assert.match(await page.eval(`${pluginRow("faint")}.textContent`), /hard to read/);
  await page.eval(`${pluginRow("faint")}.querySelector('[data-action="remove"]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await page.waitFor(`${pluginRow("faint")} == null`);
}

async function fontComesFromThePackage(page, dir) {
  const fontVar = `getComputedStyle(document.documentElement).getPropertyValue('--font-mono')`;
  assert.doesNotMatch(await page.eval(fontVar), /Platypus Theme/, "a theme not chosen loads no font");
  await install(page, dir, "typed");
  assert.ok(existsSync(join(dir, "plugins/typed/1.0.0/fonts/Typed.woff2")));
  await openSettings(page, "Appearance");
  await page.waitFor(`${card("Typed")} !== undefined`);
  await page.eval(`${card("Typed")}.click()`);
  await page.waitFor(`${bg} === '#102030'`);
  await page.waitFor(`${fontVar}.includes('Platypus Theme mono')`, 15_000);
  assert.equal(await page.eval(`[...document.fonts].some(f => f.family.includes('Platypus Theme mono') && f.status === 'loaded')`), true);
  // Another theme: the font goes.
  await page.eval(`${card("Sea")}.click()`);
  await page.waitFor(`!${fontVar}.includes('Platypus Theme')`, 15_000);
  assert.equal(await page.eval(`[...document.fonts].some(f => f.family.includes('Platypus Theme'))`), false);
}

async function turningOffFallsBack(page) {
  await openSettings(page, "Plugins");
  await page.waitFor(`${pluginRow("theme-sea")} != null`, 15_000);
  await page.eval(`${pluginRow("theme-sea")}.querySelector('[data-action="toggle"]').click()`);
  await page.waitFor(`${pluginRow("theme-sea")}.dataset.enabled === 'false'`);
  await page.waitFor(`${bg} !== '${SEA_DARK}' && ${bg} !== '#f4fbfc'`);
  await openSettings(page, "Appearance");
  await page.waitFor(`${card("Nord")} !== undefined`);
  assert.equal(await page.eval(`${card("Sea")} === undefined`), true);
  // On again, the choice holds.
  await openSettings(page, "Plugins");
  await page.waitFor(`${pluginRow("theme-sea")} != null`, 15_000);
  await page.eval(`${pluginRow("theme-sea")}.querySelector('[data-action="toggle"]').click()`);
  await page.waitFor(`${bg} === '${SEA_DARK}' || ${bg} === '#f4fbfc'`);
}

async function removingFallsBack(page, dir) {
  await page.eval(`${pluginRow("theme-sea")}.querySelector('[data-action="remove"]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await page.waitFor(`${pluginRow("theme-sea")} == null`);
  await page.waitFor(`${bg} !== '${SEA_DARK}' && ${bg} !== '#f4fbfc'`);
  assert.equal(existsSync(join(dir, "plugins/theme-sea")), false, "no file of it is left");
  await closeSettings(page);
}

/** A start with the theme chosen and the plugin's folder gone: the default, and the app works. */
async function aStartWithoutItsFilesStillStarts(page, dir) {
  await install(page, dir, "theme-sea");
  await openSettings(page, "Appearance");
  await page.waitFor(`${card("Sea")} !== undefined`);
  await page.eval(`${card("Sea")}.click()`);
  await page.waitFor(`${bg} === '${SEA_DARK}' || ${bg} === '#f4fbfc'`);
  await closeSettings(page);
  rmSync(join(dir, "plugins/theme-sea"), { recursive: true, force: true });
  await page.eval(`location.reload()`);
  await page.waitFor(`document.querySelector('.shell') !== null`, 30_000);
  await page.waitFor(`${bg} !== '' && ${bg} !== '${SEA_DARK}' && ${bg} !== '#f4fbfc'`, 15_000);
  // What it read of the setting does not break the Settings either.
  await openSettings(page, "Appearance");
  await page.waitFor(`${card("Nord")} !== undefined`);
  assert.equal(await page.eval(`${card("Sea")} === undefined`), true);
  await closeSettings(page);
}

export default async function (page, { dir }) {
  await refused(page, dir);
  await reviewSaysATheme(page, dir);
  await choosePaintsIt(page, dir);
  await warnsOnPoorContrast(page, dir);
  await fontComesFromThePackage(page, dir);
  await turningOffFallsBack(page);
  await removingFallsBack(page, dir);
  await aStartWithoutItsFilesStillStarts(page, dir);
}
