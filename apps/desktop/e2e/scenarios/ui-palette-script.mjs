// The command palette (Ctrl+K) lists the saved scripts by name: typing part of a name finds
// the script, Enter opens it with its text, a folder is not listed, a name that matches
// nothing says so, and a script renamed on disk is found by its new name.
import assert from "node:assert/strict";
import { mkdirSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { visibleTab } from "../journey-helpers.mjs";
import { editorText, sqliteConnection } from "../query.mjs";

export function setup({ dir }) {
  sqliteConnection(dir, { file: "s_fix_palette.db", id: "00000000-0000-4000-8000-0000000f5003", name: "S fix palette", sql: "SELECT 1;" });
  const scripts = join(dir, "scripts", "all");
  mkdirSync(join(scripts, "Reports"), { recursive: true });
  writeFileSync(join(scripts, "Quarterly revenue.sql"), "SELECT 'revenue'");
  writeFileSync(join(scripts, "Reports", "Zebra stripes.sql"), "SELECT 'zebra'");
}

const options = `[...document.querySelectorAll('.palette [role=option]')]`;

async function search(page, text) {
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') === document.activeElement`, 5_000);
  await page.type(text);
}

export default async function (page, { dir }) {
  await page.waitFor(`document.querySelector('.welcome') !== null || document.querySelector('.tree-row') !== null`, 15_000);

  // By part of the name, in a folder too; the folder itself is not a result.
  await search(page, "zebra");
  await page.waitFor(`${options}.some(o => o.querySelector('.palette-label').textContent.includes('Zebra stripes'))`, 10_000);
  assert.equal(await page.eval(`${options}.some(o => o.querySelector('.palette-label').textContent === 'Reports')`), false);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`, 5_000);

  // Enter opens the selected script with its text.
  await search(page, "quarterly");
  await page.waitFor(`document.querySelector('.palette li.selected .palette-label')?.textContent === 'Quarterly revenue'`, 10_000);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.palette') === null`, 5_000);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('Quarterly revenue.sql')`, 10_000);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 10_000);
  await page.waitFor(`${editorText} === "SELECT 'revenue'"`, 10_000);

  // No match: the palette says so and Enter does nothing.
  await search(page, "qqzzxx");
  await page.waitFor(`document.querySelector('.palette-empty') !== null`, 10_000);
  await page.key("Enter", "Enter", 13);
  assert.equal(await page.eval(`document.querySelectorAll('.tab').length`), 1);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`, 5_000);

  // A file renamed outside the app: the next palette lists the new name only.
  renameSync(join(dir, "scripts", "all", "Quarterly revenue.sql"), join(dir, "scripts", "all", "Annual totals.sql"));
  await search(page, "annual");
  await page.waitFor(`${options}.some(o => o.querySelector('.palette-label').textContent === 'Annual totals')`, 10_000);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`, 5_000);
  await search(page, "quarterly");
  await page.waitFor(`document.querySelector('.palette-empty') !== null`, 10_000);
  await page.key("Escape", "Escape", 27);
}
