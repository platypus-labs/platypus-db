// QA4 ui: the Keyboard shortcuts list shows an internal name as a group title.
//
// Steps: Settings > Keyboard shortcuts (any language), read the group titles.
// Expected: every group has a readable, translated title ("Everywhere", "SQL editor", ...).
// Actual: the first group is titled "dashboard.widget" (the widget menu actions of a
// dashboard: Refresh, Edit, Move up, Move down, Remove), the scope id as it is.
// Code: shell/ShortcutsPanel.tsx SCOPES has no entry for the "dashboard.widget" scope
// registered by dashboard/actions.ts, so `scopeLabel` falls back to the raw id.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Keyboard shortcuts");
  await page.waitFor(`document.querySelector('.shortcuts-scope') !== null`);
  const titles = await page.eval(`[...document.querySelectorAll('.shortcuts-scope th')].map(e => e.textContent)`);
  assert.deepEqual(titles.filter((t) => /^[a-z]+(\.[a-z]+)+$/.test(t)), [], `raw scope ids as titles: ${titles.join(", ")}`);
}
