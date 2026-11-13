// Interface zoom from the keyboard: Ctrl+= steps up the scale (100, 110, 125, 150) and stops at
// 150 %, Ctrl+- steps down (90, 80) and stops at 80 %, Ctrl+0 is back to 100 %, the keypad's and
// Shift's plus work like Ctrl+=, the Settings' Interface size follows the keys, the setting is
// saved in settings.toml (and removed again at 100 %), and a restart opens at the saved zoom.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { restart } from "../journey-helpers.mjs";
import { settingsFile, settingsLack } from "../settings.mjs";

export const ready = `document.querySelector('.shell') !== null`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

/** The page's pixel ratio relative to `base`. */
const ratio = (base) => `Math.round(window.devicePixelRatio / ${base} * 100)`;

export default async function (page, ctx) {
  await page.waitFor(`document.querySelector('.shell') !== null`);
  const base = await page.eval(`window.devicePixelRatio`);
  const at = async (percent) => {
    await page.waitFor(`${ratio(base)} === ${percent}`, 10_000);
    if (percent !== 100) await settingsFile(ctx.dir, new RegExp(`zoom = "${percent}"`));
    else await settingsLack(ctx.dir, /zoom = /);
  };

  // Up the scale, and not past the top.
  await page.key("=", "Equal", 187, CTRL);
  await at(110);
  await page.key("=", "Equal", 187, CTRL);
  await at(125);
  await page.key("=", "Equal", 187, CTRL);
  await at(150);
  await page.key("=", "Equal", 187, CTRL);
  await page.sleep(500);
  await at(150);

  // Reset, then down the scale and not past the bottom.
  await page.key("0", "Digit0", 48, CTRL);
  await at(100);
  await page.key("-", "Minus", 189, CTRL);
  await at(90);
  await page.key("-", "Minus", 189, CTRL);
  await at(80);
  await page.key("-", "Minus", 189, CTRL);
  await page.sleep(500);
  await at(80);

  // The plus of the keypad and the Shift plus of other keyboards.
  await page.key("0", "Digit0", 48, CTRL);
  await at(100);
  await page.key("+", "NumpadAdd", 107, CTRL);
  await at(110);
  await page.key("+", "Equal", 187, CTRL | 8);
  await at(125);

  // The Settings show the size the keys set, and a change there is a step for the keys.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance", "Density");
  const select = `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === 'Interface size').querySelector('select')`;
  assert.equal(await page.eval(`${select}.value`), "125");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);

  // A restart opens at the saved zoom.
  await page.key("-", "Minus", 189, CTRL);
  await at(110);
  await restart(page, ctx, { ready: `document.querySelector('.shell') !== null` });
  await page.waitFor(`${ratio(base)} === 110`, 15_000);
  await page.key("0", "Digit0", 48, CTRL);
  await at(100);
}
