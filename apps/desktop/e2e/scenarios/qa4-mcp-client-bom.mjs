// qa4-mcp-5 (low). Settings > Agents > Setup cannot install into a client's JSON file that starts with a
// UTF-8 byte-order mark: the card says "unreadable" and Install is disabled. Windows tools write that mark
// (Notepad before Windows 11 22H2, PowerShell 5.1 `Set-Content -Encoding UTF8`/`Out-File`), so a user who
// edited ~/.cursor/mcp.json by hand cannot use the one-click setup and gets no hint why.
// Steps: ~/.cursor/mcp.json = BOM + {"mcpServers":{"other":{...}}}; open Settings > Agents > Setup.
// Expected: the card is "missing" (installable); Install keeps `other`, adds `platypus`, backs the file up.
// Actual: the card is "unreadable", Install is disabled (serde_json rejects the BOM).
// Code: apps/desktop/src-tauri/src/commands/agents/clients.rs `parse_json` (and the file read in `install`/`list`):
//   strip a leading U+FEFF before `serde_json::from_str`.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { noMcp } from "../mcp.mjs";

export const skip = noMcp;
export const ready = "document.querySelector('.topbar-agents') !== null || document.querySelector('.tree-row') !== null || document.body.textContent.length > 50";

const home = (dir) => join(dir, "home");
const cursorFile = (dir) => join(home(dir), ".cursor", "mcp.json");
const client = (id) => `document.querySelector('.client-card[data-client=${JSON.stringify(id)}]')`;

export function env({ dir }) {
  return { PLATYPUS_CLIENTS_HOME: home(dir) };
}

export function setup({ dir }) {
  mkdirSync(join(home(dir), ".cursor"), { recursive: true });
  writeFileSync(cursorFile(dir), `﻿${JSON.stringify({ mcpServers: { other: { command: "npx", args: ["x"] } } }, null, 2)}\r\n`);
}

export default async function (page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Agents");
  await page.waitFor(`${client("cursor")} !== null`);
  const state = await page.eval(`${client("cursor")}.dataset.state`);
  const disabled = await page.eval(`document.querySelector('[data-install="cursor"]').disabled`);
  assert.deepEqual([state, disabled], ["missing", false], "a JSON file with a byte-order mark is installable");
}
