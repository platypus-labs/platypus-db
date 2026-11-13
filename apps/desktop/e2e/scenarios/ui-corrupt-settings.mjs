// QA ui: a settings.toml with a syntax error is ignored without a word, and nothing can be
// changed any more.
//
// Steps: settings.toml with theme = "nord", zoom = "110" and a line `[editor` (missing `]`),
// e.g. after a hand edit. Start the app, then click the theme button of the top bar.
// Expected: a message at start saying settings.toml could not be read (where, which line),
// as the connections store does for connections.toml.
// Actual: the app starts with every setting at its default (system theme, 100%, the system's
// language) and says nothing; clicking the theme button does nothing visible (the change is
// rolled back) and only the Activity log gets an "Unexpected error: invalid ...settings.toml:
// TOML parse error at line 5". The user's settings look lost.
// Code: settings.ts loadSettings() (`console.warn("settings", e)` only) and setSetting.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "en"\ntheme = "nord"\nzoom = "110"\n[editor\nformat_indent = 4\n');
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page) {
  // Give the start the time to report it.
  await page.sleep(1500);
  const shown = await page.eval(
    `[...document.querySelectorAll('.notice, .toast, .modal, [role=alert]')].map(n => n.textContent).join(' | ')`,
  );
  assert.match(shown, /settings\.toml/, "no message about the unreadable settings.toml");
}
