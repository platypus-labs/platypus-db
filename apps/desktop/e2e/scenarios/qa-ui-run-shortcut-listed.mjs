// QA ui: the editor's main shortcuts are missing from Settings > Keyboard shortcuts.
//
// Steps: Settings > Keyboard shortcuts, filter "run" (or look for Save, Open, Find,
// Autocomplete).
// Expected: README says "Every shortcut can be changed in Settings > Keyboard shortcuts", and
// lists Ctrl+Enter (run the statement), Alt+X / Ctrl+Shift+Enter (run the script), Ctrl+S /
// Ctrl+O (save / open), Ctrl+F, Ctrl+Space: they should be listed and changeable.
// Actual: only "Run in a new result tab" and "Run script with the command-line client" are
// listed; Run statement, Run script, Save, Open, Find and Autocomplete are fixed CodeMirror
// keys that cannot be seen nor changed.
// Code: editor/SqlEditor.tsx keymap.of([{ key: "Mod-Enter" ... }, { key: "Alt-x" ... },
// { key: "Mod-s" ... }]) instead of registered "editor" actions.
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
  await page.waitFor(`document.querySelectorAll('.shortcuts tr').length > 20`);
  const keys = await page.eval(`[...document.querySelectorAll('.shortcuts .shortcut-key')].map(b => b.textContent)`);
  for (const k of ["Ctrl+Enter", "Alt+X", "Ctrl+S", "Ctrl+O"]) {
    assert.ok(keys.includes(k), `${k} is not in the list of shortcuts`);
  }
}
