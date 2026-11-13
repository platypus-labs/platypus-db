// QA4 ui: a shortcut that contains Delete or Backspace cannot be assigned.
//
// Steps: Settings > Keyboard shortcuts, click the shortcut of "Zoom in", press Ctrl+Delete
// (same with Ctrl+Backspace, or Shift+Delete, the default of "Delete the row" in the grid).
// Expected: "Zoom in" becomes Ctrl+Delete: only the bare Backspace/Delete removes a shortcut.
// Actual: the shortcut of "Zoom in" is removed ("—"), whatever modifiers are held, so a
// combination with Delete or Backspace can never be recorded (the Shift+Delete default of the
// grid cannot be put back after another key was chosen).
// Code: shell/ShortcutsPanel.tsx record(): `e.key === "Backspace" || e.key === "Delete"` is
// tested before the modifiers are looked at.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

const key = (title) =>
  `[...document.querySelectorAll('.shortcuts tr')].find(r => r.querySelector('td')?.textContent === ${JSON.stringify(title)})?.querySelector('.shortcut-key')`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Keyboard shortcuts");
  await page.waitFor(`${key("Zoom in")} != null`);
  await page.eval(`${key("Zoom in")}.click(); ${key("Zoom in")}.focus()`);
  await page.waitFor(`${key("Zoom in")}.textContent.startsWith('Press')`);
  await page.key("Delete", "Delete", 46, CTRL);
  await page.sleep(300);
  assert.equal(await page.eval(`${key("Zoom in")}.textContent`), "Ctrl+Delete", "Ctrl+Delete was not recorded");
}
