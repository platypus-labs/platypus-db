// QA5 mcp (low): Settings > Agents > Install rewrites the numbers of the client's file it does not touch.
//
// Steps: ~/.cursor/mcp.json holds `"big": 123456789012345678901234567890` (and `"e": 1E5`) next to its
// servers; Install PlatypusDB there (the dialog promises "A copy of the file as it is now is saved beside
// it first" and docs/user/AGENTS.md "keeping the rest of the file as it was").
// Expected: the other keys are written as read: the 30-digit integer stays 123456789012345678901234567890.
// Actual: `"big": 1.2345678901234568e+29` (and `1E5` becomes `100000.0`): the file is parsed to f64 and
// printed again, so a value over u64 silently loses its digits (the backup keeps the original).
// Code: apps/desktop/src-tauri/src/commands/agents/clients.rs `with_entry` (serde_json Value without the
// `arbitrary_precision` feature, then `to_string_pretty`).
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { noMcp } from "../mcp.mjs";

export const skip = noMcp;
export const ready = "document.body.textContent.length > 50";
const home = (dir) => join(dir, "home");
const file = (dir) => join(home(dir), ".cursor", "mcp.json");

export function env({ dir }) {
  return { PLATYPUS_CLIENTS_HOME: home(dir) };
}

export function setup({ dir }) {
  mkdirSync(join(home(dir), ".cursor"), { recursive: true });
  writeFileSync(file(dir), `{\n  "big": 123456789012345678901234567890,\n  "mcpServers": {"other": {"command": "npx"}}\n}\n`);
}

export default async function (page, { dir }) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Agents");
  await page.waitFor(`document.querySelector('[data-install="cursor"]') !== null`);
  await page.eval(`document.querySelector('[data-install="cursor"]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent.trim() === 'Install').click()`);
  await page.waitFor(`document.querySelector('.client-card[data-client=cursor]')?.dataset.state === 'installed'`, 10_000);
  const text = readFileSync(file(dir), "utf8");
  assert.match(text, /"big": 123456789012345678901234567890\b/, `the integer is kept: ${text.slice(0, 80)}`);
}
