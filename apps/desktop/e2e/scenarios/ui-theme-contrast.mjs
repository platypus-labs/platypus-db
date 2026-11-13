// QA ui: secondary text is hard to read in Solarized Light, Nord and Classic Light.
//
// Steps: Settings > Appearance, pick each theme; measure the contrast of --fg-muted (hints,
// descriptions, the navigator's counts, the status bar) on the surfaces it sits on.
// Expected: at least 4.5:1 (WCAG AA for normal text), as theme-sqlite.mjs already requires
// of Platypus Light.
// Actual: Solarized Light 3.51 on the background, 3.09 on the sidebar and grid header, 3.22 on
// the tab bar; Nord 4.05 on the sidebar and grid header; Classic Light 4.35-4.43. Solarized
// Light's accent text on the accent is 3.68.
// Code: theme.ts PRESETS (solarized_light, nord, light) "fg-muted" and "accent-fg".
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

const rgbOf = (name) => `(() => {
  const probe = document.createElement('div');
  probe.style.color = 'var(${name})';
  document.body.append(probe);
  const c = getComputedStyle(probe).color;
  probe.remove();
  const n = c.match(/[0-9.]+/g).map(Number);
  return JSON.stringify(c.startsWith('color(') ? n.slice(0, 3).map((v) => v * 255) : n.slice(0, 3));
})()`;
const luminance = ([r, g, b]) => {
  const lin = (v) => ((v /= 255) <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
};
const contrast = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance");
  const low = [];
  for (const label of ["Solarized Light", "Nord", "Classic Light"]) {
    await page.eval(`[...document.querySelectorAll('.theme-card')].find(c => c.textContent === ${JSON.stringify(label)}).click()`);
    await page.waitFor(`document.querySelector('.theme-card.selected')?.textContent === ${JSON.stringify(label)}`);
    await page.sleep(100);
    for (const [fg, bg] of [["--fg-muted", "--bg"], ["--fg-muted", "--bg-sidebar"], ["--fg-muted", "--bg-tabbar"], ["--fg-muted", "--grid-header"], ["--accent-fg", "--accent"]]) {
      const ratio = contrast(JSON.parse(await page.eval(rgbOf(fg))), JSON.parse(await page.eval(rgbOf(bg))));
      if (ratio < 4.5) low.push(`${label}: ${fg} on ${bg} ${ratio.toFixed(2)}`);
    }
  }
  assert.deepEqual(low, []);
}
