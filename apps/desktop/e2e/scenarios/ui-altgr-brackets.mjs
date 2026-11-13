// QA5 ui: on keyboards where `[` and `]` are typed with AltGr (Italian, German, Spanish, French...),
// the SQL editor cannot type them: AltGr reaches the page as Ctrl+Alt, which is the default shortcut
// of "Fold all" (Ctrl+Alt+[) and "Unfold all" (Ctrl+Alt+]).
//
// Steps: in the SQL editor of a connection, with the Italian keyboard layout, press AltGr+è (key "[",
// reported by Windows as ctrl+alt with the text "[") and AltGr++ ("]"), as when writing `arr[1]`.
// Expected: `[` and `]` are typed. Actual: the key is taken by the fold shortcut (default prevented),
// nothing is typed; the user cannot write `[` or `]` in a query at all.
// Code: apps/desktop/src/editor/actions.ts `command("fold_all", ..., "Mod+Alt+[")` and unfold_all
// "Mod+Alt+]"; actions.ts `matches` has no AltGraph handling (e.getModifierState("AltGraph")).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL } from "../cdp.mjs";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const treeRow = (label) => `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  new DatabaseSync(join(dir, "a.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 1\n[[connections]]\nid = "00000000-0000-4000-8000-000000000e52"\nname = "Alt db"\ndriver = "sqlite"\ndatabase = '${join(dir, "a.db")}'\nsave_password = false\n`);
}

/** A key typed with AltGr: Windows reports Ctrl+Alt, and the key's text is the character. */
async function altGr(page, key, code, keyCode) {
  await page.send("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode: keyCode, modifiers: CTRL | ALT, text: key, unmodifiedText: key });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: keyCode, modifiers: CTRL | ALT });
}

export default async function (page) {
  await page.eval(`${treeRow("Alt db")}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("select ");
  // Control: an AltGr character no shortcut takes is typed (the harness sends what Windows does).
  await altGr(page, "@", "Semicolon", 186);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === 'select @'`);
  await page.key("Backspace", "Backspace", 8);
  await page.type("a");
  await altGr(page, "[", "BracketLeft", 219);
  await page.type("1");
  await altGr(page, "]", "BracketRight", 221);
  await page.sleep(300);
  const text = await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`);
  assert.equal(text, "select a[1]", "AltGr+[ and AltGr+] type the brackets");
}

