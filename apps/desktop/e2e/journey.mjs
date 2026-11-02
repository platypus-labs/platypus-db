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
 *  (a text left untranslated), and if a label is cut. `allow` lists texts that may stay;
 *  `cutOk` the parts of a cut label's description that may be cut. */
export async function screenIsItalian(page, mode, where, allow = [], cutOk = []) {
  if (languageOf(mode) !== "it") return;
  const dict = italian();
  const { texts: shown, cut } = await page.eval(pageTexts);
  const english = shown
    .map((s) => s.replace(/^@[a-z-]+: /, ""))
    .filter((s) => dict[s] !== undefined && dict[s] !== s && !allow.includes(s));
  assert.deepEqual(english, [], `${where}: English left on an Italian screen`);
  // A navigator row count gives way first when the row is narrow: cut by design.
  const byDesign = ["span.tree-count", ...cutOk];
  assert.deepEqual(cut.filter((c) => !byDesign.some((ok) => c.includes(ok))), [], `${where}: texts cut`);
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

/** The focus is in a SQL editor, which Tab would indent. The field of "SQL on the result" is one
 *  line: Tab leaves it, and Escape would close the SQL view. */
const inEditor = `!!document.activeElement?.closest?.('.cm-content') && !document.activeElement.closest('.result-sql')`;

/** One Tab press (Shift+Tab when `back`) that waits for the focus to move. It leaves an editor
 *  first (Escape twice: the first may be taken by the completion list). With `attempts` > 1 a
 *  Tab that moved nothing is pressed again. */
export async function tabStep(page, { back = false, attempts = 1, wait = 5_000 } = {}) {
  await page.eval(`void (window.__focusBefore = document.activeElement)`);
  for (let attempt = 1; ; attempt++) {
    if (await page.eval(inEditor)) {
      await keys.escape(page);
      await keys.escape(page);
    }
    await keys.tab(page, back ? 8 : 0);
    try {
      await page.waitFor(`document.activeElement !== window.__focusBefore`, wait);
      return;
    } catch (e) {
      if (attempt >= attempts) throw new Error(`${e.message}; the focus stays on ${await page.eval(`document.activeElement.outerHTML.slice(0, 160)`)}`);
    }
  }
}

/** The space bar: it types a space, so it also toggles a focused checkbox. */
export const space = (page) => press(page, " ", "Space", 32).then(() => {});

/** The focused element shows a focus ring (outline or box shadow), as the keyboard user sees it. */
export const focusVisible = `(() => {
  const el = document.activeElement;
  if (!el || el === document.body) return false;
  const s = getComputedStyle(el);
  return (s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) > 0) || s.boxShadow !== 'none';
})()`;
