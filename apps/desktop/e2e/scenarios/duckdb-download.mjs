// DuckDB downloaded at first use: the profile starts without DuckDB's library, so opening a
// DuckDB file shows the popup, which downloads it from DuckDB's GitHub release; the query
// then runs, and Settings › Drivers lists the library and removes it. Needs the network.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const NAME = "Local duck";
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const popup = `document.querySelector('.driver-popup')`;
const button = (scope, text) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const settingsRow = `document.querySelector('.modal.settings [data-vendor-driver="duckdb"]')`;

/** The runner leaves the library out of this profile. */
export const duckdb = false;

export async function skip() {
  try {
    await fetch("https://github.com", { method: "HEAD", signal: AbortSignal.timeout(5000) });
    return null;
  } catch {
    return "no network to download DuckDB";
  }
}

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 6

[[connections]]
id = "00000000-0000-4000-8000-000000000081"
name = "${NAME}"
driver = "duckdb"
database = ${JSON.stringify(join(dir, "local.duckdb"))}
`,
  );
}

export default async function (page) {
  await page.waitFor(`window.__platypus !== undefined && ${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  const rows = await page.eval(`document.querySelectorAll('.tree-row').length`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${popup}?.dataset.vendor === 'duckdb'`, 30_000);
  await page.waitFor(`${popup}.querySelector('h2').textContent === 'Download DuckDB?'`);
  const text = await page.eval(`${popup}.textContent`);
  assert.match(text, /DuckDB · 2\.0\.0 · \d+\.\d MB/);
  assert.ok(text.includes("License: MIT"), text);
  assert.equal(await page.eval(`${popup}.querySelector('[data-notice="uac"]')`), null);
  await page.eval(`${button(popup, "Download and connect")}.click()`);
  await page.waitFor(`${popup} === null`, 300_000);
  const tree = `[...document.querySelectorAll('.tree-row')]`;
  await page.waitFor(`${tree}.length > ${rows} && !${tree}.some(r => r.textContent.includes('loading…'))`, 60_000);

  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`, 30_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT 42 AS answer");
  await page.waitFor(`${visibleTab}.querySelector('.cm-line')?.textContent === 'SELECT 42 AS answer'`);
  await page.eval(`${visibleTab}.querySelectorAll('.statusbar span').forEach(s => (s.textContent = ''))`);
  await page.key("Enter", "Enter", 13, 2);
  await page.waitFor(`${visibleTab}.querySelector('.statusbar span')?.textContent === '1 row'`, 60_000);

  // Settings › Drivers shows the library and removes it (deleted at the next start, as
  // Windows keeps a loaded library).
  await page.eval(`document.querySelector('button[aria-label="Settings"]').click()`);
  await page.settingsTab("Drivers");
  await page.waitFor(`${settingsRow}?.querySelector('[data-driver-state="installed"]') != null`, 15_000);
  const listed = await page.eval(`${settingsRow}.textContent`);
  assert.ok(listed.includes("DuckDB 2.0.0 installed"), listed);
  await page.eval(`${button(settingsRow, "Remove")}.click()`);
  await page.waitFor(`${settingsRow}.querySelector('[data-driver-state="missing"]') != null`, 15_000);
}
