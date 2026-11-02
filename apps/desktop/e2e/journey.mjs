// What the complete user journeys (s-jrn-*) share. Each journey runs three times: in English,
// in Italian and with the keyboard only. `mode` is "en", "it" or "keys" (the keyboard run is in
// English); `t` gives the text the interface shows for an English label in the run's language.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { italian, pageTexts } from "./qa-jrn.mjs";

/** The interface language of a run. */
export const languageOf = (mode) => (mode === "it" ? "it" : "en");

/** Sets the interface language in the profile, before the app starts (a scenario's setup). */
export function setLanguage(dir, mode) {
  const file = join(dir, "settings.toml");
  const text = existsSync(file) ? readFileSync(file, "utf8").replace(/^language\s*=.*\n/m, "") : "";
  const line = `language = "${languageOf(mode)}"`;
  writeFileSync(file, text.includes("[appearance]") ? text.replace("[appearance]", `[appearance]\n${line}`) : `[appearance]\n${line}\n\n${text}`);
}

/** `t(english)`: the label as the run shows it. A label the Italian dictionary lacks fails the
 *  run, so a journey never passes by looking for English text on an Italian screen. */
export function texts(mode) {
  if (languageOf(mode) === "en") return (english) => english;
  const dict = italian();
  return (english) => {
    assert.ok(english in dict, `no Italian for ${JSON.stringify(english)} in src/i18n/it.ts`);
    return dict[english];
  };
}

/** On an Italian run, fails if the screen shows an English label that has an Italian one
 *  (a text left untranslated), and if a label is cut. `allow` lists texts that may stay. */
export async function screenIsItalian(page, mode, where, allow = []) {
  if (languageOf(mode) !== "it") return;
  const dict = italian();
  const { texts: shown, cut } = await page.eval(pageTexts);
  const english = shown
    .map((s) => s.replace(/^@[a-z-]+: /, ""))
    .filter((s) => dict[s] !== undefined && dict[s] !== s && !allow.includes(s));
  assert.deepEqual(english, [], `${where}: English left on an Italian screen`);
  assert.deepEqual(cut, [], `${where}: texts cut`);
}

/** A key as the keyboard sends it: Enter also types its character, so it presses buttons. */
export async function press(page, key, code, keyCode, modifiers = 0) {
  const text = key === "Enter" && !modifiers ? "\r" : undefined;
  await page.send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", key, code, windowsVirtualKeyCode: keyCode, modifiers, text, unmodifiedText: text });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: keyCode, modifiers });
}

export const keys = {
  tab: (page, modifiers = 0) => press(page, "Tab", "Tab", 9, modifiers),
  enter: (page, modifiers = 0) => press(page, "Enter", "Enter", 13, modifiers),
  escape: (page) => press(page, "Escape", "Escape", 27),
  up: (page) => press(page, "ArrowUp", "ArrowUp", 38),
  down: (page) => press(page, "ArrowDown", "ArrowDown", 40),
  left: (page) => press(page, "ArrowLeft", "ArrowLeft", 37),
  right: (page) => press(page, "ArrowRight", "ArrowRight", 39),
};

/** Presses Tab until `test` (a page expression) holds; each press waits for the focus to move. */
export async function tabUntil(page, test, max = 40) {
  for (let i = 0; i < max; i++) {
    if (await page.eval(test)) return;
    await page.eval(`void (window.__focusBefore = document.activeElement)`);
    await keys.tab(page);
    await page.waitFor(`document.activeElement !== window.__focusBefore`, 5_000);
  }
  throw new Error(`Tab never reached ${test}`);
}

/** The focused element shows a focus ring (outline or box shadow), as the keyboard user sees it. */
export const focusVisible = `(() => {
  const el = document.activeElement;
  if (!el || el === document.body) return false;
  const s = getComputedStyle(el);
  return (s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) > 0) || s.boxShadow !== 'none';
})()`;
