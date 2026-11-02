// Renaming a saved script (or its folder) in the Scripts panel must move the editor tabs open
// on it: the title follows the new name and Ctrl+S writes the new file, never the old path.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { visibleTab } from "../qa-jrn.mjs";
import { editorText, sqliteConnection } from "../query.mjs";

export function setup({ dir }) {
  sqliteConnection(dir, { file: "s_fix_rename.db", id: "00000000-0000-4000-8000-0000000f5001", name: "S fix rename", sql: "SELECT 1;" });
  const scripts = join(dir, "scripts", "all");
  mkdirSync(join(scripts, "Sub"), { recursive: true });
  writeFileSync(join(scripts, "Alpha.sql"), "SELECT 'alpha'");
  writeFileSync(join(scripts, "Sub", "Beta.sql"), "SELECT 'beta'");
}

const row = (path) => `[...document.querySelectorAll('.script-row')].find(r => r.dataset.path === ${JSON.stringify(path)})`;
const activeTab = `document.querySelector('.tab.active')?.textContent.replace(/×$/, '')`;

async function rename(page, path, name) {
  await page.waitFor(`${row(path)} !== undefined`, 10_000);
  await page.eval(`${row(path)}.focus()`);
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.script-rename') === document.activeElement`, 5_000);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(name);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${row(path)} === undefined`, 10_000);
}

async function openAndEdit(page, path, base, suffix) {
  await page.eval(`${row(path)}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${activeTab}?.startsWith(${JSON.stringify(base)})`, 10_000);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 10_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("End", "End", 35, CTRL);
  await page.type(suffix);
  await page.waitFor(`${editorText}.endsWith(${JSON.stringify(suffix)})`, 5_000);
}

export default async function (page, { dir }) {
  const scripts = join(dir, "scripts", "all");
  const files = (sub = "") => readdirSync(join(scripts, sub)).filter((f) => !f.startsWith(".")).sort().join(",");

  // A script renamed while open: the tab takes the new name, Ctrl+S writes the new file.
  await openAndEdit(page, "Alpha.sql", "Alpha.sql", " -- one");
  await rename(page, "Alpha.sql", "Gamma");
  await page.waitFor(`${activeTab}?.startsWith("Gamma.sql")`, 10_000);
  assert.equal(await page.eval(`${activeTab}.startsWith("Alpha")`), false);
  await page.eval(`${visibleTab}.querySelector(".cm-content").focus()`);
  await page.key("s", "KeyS", 83, CTRL);
  const gamma = join(scripts, "Gamma.sql");
  const deadline = Date.now() + 10_000;
  while (!(existsSync(gamma) && readFileSync(gamma, "utf8").endsWith("-- one")) && Date.now() < deadline) await page.sleep(200);
  assert.equal(readFileSync(gamma, "utf8"), "SELECT 'alpha' -- one");
  assert.equal(files(), "Gamma.sql,Sub", "Ctrl+S recreated the old file");

  // A folder renamed: the tab of a script inside it follows to the new folder.
  await page.waitFor(`${row("Sub/Beta.sql")} !== undefined`, 10_000);
  await openAndEdit(page, "Sub/Beta.sql", "Beta.sql", " -- two");
  await rename(page, "Sub", "Moved");
  await page.waitFor(`${row("Moved/Beta.sql")} !== undefined`, 10_000);
  await page.eval(`${visibleTab}.querySelector(".cm-content").focus()`);
  await page.key("s", "KeyS", 83, CTRL);
  const beta = join(scripts, "Moved", "Beta.sql");
  const limit = Date.now() + 10_000;
  while (!(existsSync(beta) && readFileSync(beta, "utf8").endsWith("-- two")) && Date.now() < limit) await page.sleep(200);
  assert.equal(readFileSync(beta, "utf8"), "SELECT 'beta' -- two");
  assert.equal(files(), "Gamma.sql,Moved", "the old folder came back");
  assert.equal(files("Moved"), "Beta.sql");
  // The tab of the other script is untouched.
  assert.equal(await page.eval(`[...document.querySelectorAll('.tab')].length`), 2);
}
