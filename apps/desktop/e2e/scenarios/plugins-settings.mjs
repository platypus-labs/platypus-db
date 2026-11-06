// A plugin's settings page (Settings > Plugins, examples/plugins/settings-demo). The review names
// the fields and which go to the keychain; the page draws one field of each type from the
// manifest, with its default; what is saved reaches the module through its action; a value out of
// range is not saved; a secret is in no file of the data folder and the page never shows it back,
// only that it is set; a key the manifest does not declare is refused to the module; Reset and
// Clear go back; values outlive the dialog and go with the plugin when it is removed.
//
// Values of another plugin and the host function's refusals are tested in Rust (`platypus-plugins`).
import assert from "node:assert/strict";
import { cpSync, existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { setValue } from "../admin-users.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const EXAMPLES = resolve(here, "../../../../examples/plugins");
const NAME = "E2E plugin settings";
const ID = "settings-demo";
const SECRET = "s3cret-value";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;
const panel = `document.querySelector('.plugins-panel')`;
const pluginRow = `${panel}?.querySelector('.driver-block[data-plugin="${ID}"]:not(.plugin-review)')`;
const review = `${panel}?.querySelector('[data-field="plugin-review"]')`;
const form = `${pluginRow}?.querySelector('[data-field="plugin-settings"]')`;
const rowSelector = (key) => `.plugins-panel .setting[data-key="${ID}.${key}"]`;
const row = (key) => `document.querySelector('${rowSelector(key)}')`;
const toasts = `[...document.querySelectorAll('.toast')].map(t => t.textContent).join(' | ')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "settings.db"));
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY);`);
  db.close();
  cpSync(join(EXAMPLES, ID), join(dir, "packages", ID), { recursive: true });
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 8

[[connections]]
id = "00000000-0000-4000-8000-0000000e0d01"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "settings.db")}'
save_password = false
`,
  );
  writeFileSync(join(dir, "settings.toml"), `version = 1\n\n[drivers]\nauto_update = false\n`);
}

async function openPluginsPage(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Plugins");
  await page.waitFor(`${panel} !== null`, 15_000);
}

async function closeSettings(page) {
  await page.eval(`document.querySelector('.modal.settings [role="tab"][aria-selected="true"]')?.focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

async function install(page, dir) {
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(join(dir, "packages", ID))})`);
  await page.eval(`${panel}.querySelector('[data-action="install-folder"]').click()`);
  await page.waitFor(`${review} !== null`, 15_000);
}

async function reviewsAndInstalls(page, dir) {
  await openPluginsPage(page);
  await install(page, dir);
  const text = await page.eval(`${review}.textContent`);
  assert.match(text, /Its own settings: Endpoint, Retries, Verbose, Mode, API token/);
  assert.match(text, /Kept in the system's keychain: API token/);
  // No page until it is installed.
  assert.equal(await page.eval(`${review}.querySelector('[data-action="settings"]') === null`), true);
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow} != null && ${review} === null`, 15_000);
}

async function openForm(page) {
  await page.eval(`${pluginRow}.querySelector('[data-action="settings"]').click()`);
  await page.waitFor(`${form} != null && ${row("token")} != null`, 15_000);
}

/** The module's message for the action `title`, run from the editor's Plugins submenu. */
async function runAction(page, title) {
  await page.eval(`document.querySelectorAll('.toast').forEach(t => t.click())`);
  await page.waitFor(`document.querySelectorAll('.toast').length === 0`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').id = 'e2e-editor'`);
  const r = await page.rect("#e2e-editor");
  await page.click(r.x + 40, r.y + 10, { button: "right" });
  await page.waitFor(`${menuButton("Plugins")} !== undefined`);
  await page.eval(`${menuButton("Plugins")}.click()`);
  await page.waitFor(`${menuButton(title)} !== undefined`);
  await page.eval(`${menuButton(title)}.click()`);
  await page.waitFor(`(${toasts}) !== ''`, 15_000);
  return page.eval(toasts);
}

/** Types in a field whose value saves on blur, and leaves it. */
async function saveField(page, key, value) {
  const input = `${rowSelector(key)} input`;
  await page.eval(`document.querySelector('${input}').focus()`);
  await setValue(page, input, value);
  await page.eval(`document.querySelector('${input}').blur()`);
}

/** Every file under `folder`, as text. */
function filesUnder(folder) {
  return readdirSync(folder).flatMap((name) => {
    const path = join(folder, name);
    return statSync(path).isDirectory() ? filesUnder(path) : [[path, readFileSync(path, "latin1")]];
  });
}

const settingsFile = (dir) => join(dir, "plugins", "settings.toml");

/** The plugins' settings file, once `done(text)` holds. */
async function settingsText(dir, done) {
  const deadline = Date.now() + 10_000;
  let text = "";
  while (Date.now() < deadline) {
    text = existsSync(settingsFile(dir)) ? readFileSync(settingsFile(dir), "utf8") : "";
    if (done(text)) return text;
    await new Promise((r) => setTimeout(r, 100));
  }
  assert.fail(`not as expected:\n${text}`);
}

async function showsDefaults(page) {
  assert.equal(await page.eval(`document.querySelector('${rowSelector("endpoint")} input').value`), "https://example.com");
  assert.equal(await page.eval(`document.querySelector('${rowSelector("retries")} input').value`), "3");
  assert.equal(await page.eval(`document.querySelector('${rowSelector("verbose")} input').checked`), false);
  assert.equal(await page.eval(`document.querySelector('${rowSelector("mode")} select').value`), "fast");
  assert.equal(await page.eval(`${row("token")}.dataset.secret`), "unset");
  assert.equal(await page.eval(`document.querySelector('${rowSelector("token")} input').type`), "password");
  assert.match(await page.eval(`${row("endpoint")}.textContent`), /Where the plugin would send its requests\./);
}

async function savesEachType(page, dir) {
  await saveField(page, "endpoint", "https://x.example");
  await settingsText(dir, (t) => t.includes("https://x.example"));

  // Out of range: not saved, the field says so.
  await saveField(page, "retries", "99");
  await page.waitFor(`document.querySelector('${rowSelector("retries")} input[aria-invalid="true"]') !== null`, 5000);
  assert.doesNotMatch(await settingsText(dir, () => true), /retries/);
  await saveField(page, "retries", "7");
  await settingsText(dir, (t) => /retries = 7/.test(t));

  await page.eval(`document.querySelector('${rowSelector("verbose")} input').click()`);
  await settingsText(dir, (t) => /verbose = true/.test(t));
  await setValue(page, `${rowSelector("mode")} select`, "safe");
  await settingsText(dir, (t) => /mode = "safe"/.test(t));

  await saveField(page, "token", SECRET);
  await page.waitFor(`${row("token")}.dataset.secret === 'set'`, 10_000);
  assert.equal(await page.eval(`document.querySelector('${rowSelector("token")} input').value`), "", "the page does not keep it");
  assert.equal(await page.eval(`document.querySelector('${rowSelector("token")} input').placeholder`), "Saved in the keychain");
  // In no file of the data folder.
  for (const [path, text] of filesUnder(dir)) {
    assert.ok(!text.includes(SECRET), `${path} holds the secret`);
  }
}

async function moduleReadsThem(page) {
  await closeSettings(page);
  const shown = await runAction(page, "Show my settings");
  assert.match(shown, /endpoint="https:\/\/x\.example" retries=7 verbose=true mode="safe" token: 12 characters/);
  // What the manifest does not declare is refused, and the action goes on.
  const other = await runAction(page, "Read a setting of someone else");
  assert.match(other, /Refused: not allowed/);
  assert.ok(!other.includes(SECRET));
}

async function goesBack(page, dir) {
  // Values outlive the dialog.
  await openPluginsPage(page);
  await openForm(page);
  assert.equal(await page.eval(`document.querySelector('${rowSelector("endpoint")} input').value`), "https://x.example");
  assert.equal(await page.eval(`document.querySelector('${rowSelector("mode")} select').value`), "safe");
  assert.equal(await page.eval(`${row("token")}.dataset.secret`), "set");

  // Reset and Clear.
  await page.eval(`[...${row("endpoint")}.querySelectorAll('button')].find(b => b.textContent === 'Reset').click()`);
  await settingsText(dir, (t) => !t.includes("https://x.example"));
  await page.waitFor(`document.querySelector('${rowSelector("endpoint")} input').value === 'https://example.com'`);
  await page.eval(`${row("token")}.querySelector('[data-action="clear-secret"]').click()`);
  await page.waitFor(`${row("token")}.dataset.secret === 'unset'`, 10_000);
  await closeSettings(page);
  assert.match(await runAction(page, "Show my settings"), /endpoint="https:\/\/example\.com" .* token: not set/);
}

async function goesWithThePlugin(page, dir) {
  await openPluginsPage(page);
  await openForm(page);
  await saveField(page, "token", SECRET);
  await page.waitFor(`${row("token")}.dataset.secret === 'set'`, 10_000);
  await page.eval(`${pluginRow}.querySelector('[data-action="remove"]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger, .modal.confirm button.primary').click()`);
  await page.waitFor(`${pluginRow} == null`);
  assert.doesNotMatch(await settingsText(dir, () => true), /retries|mode|verbose/);

  // A new install starts clean, the keychain's secret included.
  await install(page, dir);
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow} != null && ${review} === null`, 15_000);
  await openForm(page);
  assert.equal(await page.eval(`${row("token")}.dataset.secret`), "unset");
  assert.equal(await page.eval(`document.querySelector('${rowSelector("mode")} select').value`), "fast");
  // Leaves nothing in the keychain of the machine.
  await page.eval(`${pluginRow}.querySelector('[data-action="remove"]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger, .modal.confirm button.primary').click()`);
  await page.waitFor(`${pluginRow} == null`);
  await closeSettings(page);
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 20_000);

  await reviewsAndInstalls(page, dir);
  await openForm(page);
  await showsDefaults(page);
  await savesEachType(page, dir);
  await moduleReadsThem(page);
  await goesBack(page, dir);
  await goesWithThePlugin(page, dir);
}
