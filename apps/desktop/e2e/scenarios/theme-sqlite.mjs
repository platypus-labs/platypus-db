// The Appearance settings: a theme preset, a custom accent color, grid density and interface
// size apply at once and are saved; the sidebar button cycles system, Platypus Light and Dark.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

const cssVar = (name) => `getComputedStyle(document.documentElement).getPropertyValue(${JSON.stringify(name)}).trim()`;
const setting = (label) =>
  `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

/** Sets a React-controlled input or select, as typing or picking would. */
function change(selector, value) {
  return `(() => {
    const el = ${selector};
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  })()`;
}

async function saved(dir, pattern) {
  const deadline = Date.now() + 5000;
  let text = "";
  while (!pattern.test(text) && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
    try {
      text = readFileSync(join(dir, "settings.toml"), "utf8");
    } catch {
      // Not written yet.
    }
  }
  assert.match(text, pattern);
}

// No saved connections: there is no navigator row to wait for.
export const ready = `document.querySelector('.shell') !== null`;

export default async function (page, { dir, shots }) {
  await page.waitFor(`document.querySelector('.shell') !== null`);

  // The bundled fonts: Inter for the interface, JetBrains Mono for SQL and the grid.
  // `document.fonts.check` is true for a family with no faces at all, so look for the faces.
  const loaded = (family) =>
    `[...document.fonts].some(f => f.family.replace(/"/g, '') === ${JSON.stringify(family)} && f.status === 'loaded')`;
  await page.waitFor(`document.fonts.status === 'loaded'`);
  await page.waitFor(loaded("Inter"));
  // Nothing on this screen is monospace yet, and faces load on first use: ask for it.
  await page.eval(`document.fonts.load('12px "JetBrains Mono"').then(() => true)`);
  await page.waitFor(loaded("JetBrains Mono"));
  assert.match(await page.eval(`getComputedStyle(document.body).fontFamily`), /^"?Inter/);
  assert.match(await page.eval(cssVar("--font-mono")), /JetBrains Mono/);

  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  // Appearance comes first.
  assert.equal(await page.eval(`document.querySelector('.modal.settings h3').textContent`), "Appearance");

  // First start, theme "System": one of the two Platypus themes, whatever Windows uses.
  const css = (name) => page.eval(cssVar(name));
  assert.ok(["#141519", "#ffffff"].includes(await css("--bg")), await css("--bg"));

  // Every preset sets the variables added with the Platypus themes.
  const pick = (label) =>
    page.eval(`[...document.querySelectorAll('.theme-card')].find(c => c.textContent === ${JSON.stringify(label)}).click()`);
  const NEW_VARS = ["--bg-rail", "--bg-tabbar", "--fg-strong", "--border-strong", "--active",
    "--obj-db", "--obj-table", "--obj-view", "--obj-routine", "--obj-trigger",
    "--type-dev", "--type-dev-bg", "--type-test", "--type-test-bg", "--type-prod", "--type-prod-bg"];
  const labels = JSON.parse(await page.eval(`JSON.stringify([...document.querySelectorAll('.theme-card')].map(c => c.textContent))`));
  assert.ok(labels.includes("Platypus Dark") && labels.includes("Classic Dark"), labels.join(", "));
  for (const label of labels) {
    await pick(label);
    await page.waitFor(`document.querySelector('.theme-card.selected')?.textContent === ${JSON.stringify(label)}`);
    for (const v of NEW_VARS) assert.notEqual(await css(v), "", `${label} ${v}`);
  }

  await pick("Platypus Dark");
  await page.waitFor(`${cssVar("--bg")} === '#141519'`);
  assert.deepEqual(
    [await css("--bg-sidebar"), await css("--bg-rail"), await css("--border"), await css("--accent")],
    ["#17181d", "#0d0e11", "#23252c", "#6c63ff"],
  );
  await pick("Classic Dark");
  await page.waitFor(`${cssVar("--bg")} === '#1b1d21'`); // the former "Dark"

  // A custom background on Platypus Dark: the colors derived from it follow.
  await pick("Platypus Dark");
  await page.eval(change(`${setting("Background")}.querySelector('input[type=color]')`, "#202020"));
  await page.waitFor(`${cssVar("--bg")} === '#202020'`);
  assert.notEqual(await css("--bg-rail"), "#0d0e11");
  assert.notEqual(await css("--bg-tabbar"), "#0f1013");
  assert.notEqual(await css("--active"), "#24262e");
  await page.eval(`[...${setting("Background")}.querySelectorAll('button')].find(b => b.textContent === 'Reset').click()`);
  await page.waitFor(`${cssVar("--bg")} === '#141519'`);

  await page.eval(`[...document.querySelectorAll('.theme-card')].find(c => c.textContent === 'Nord').click()`);
  await page.waitFor(`${cssVar("--bg")} === '#2e3440'`);
  assert.equal(await page.eval(`document.documentElement.style.colorScheme`), "dark");
  await saved(dir, /\[appearance\][^[]*theme = "nord"/);
  if (shots) {
    await page.eval(`document.querySelector('.settings-body').scrollTop = 0`);
    await page.screenshot(join(shots, "theme-cards.png"));
  }

  await page.eval(change(`${setting("Accent color")}.querySelector('input[type=color]')`, "#ff6600"));
  await page.waitFor(`${cssVar("--accent")} === '#ff6600'`);
  assert.equal(await page.eval(cssVar("--accent-fg")), "#000000");

  await page.eval(change(`${setting("Grid rows")}.querySelector('select')`, "compact"));
  await page.waitFor(`${cssVar("--grid-row-h")} === '20'`);

  const before = await page.eval(`window.devicePixelRatio`);
  await page.eval(change(`${setting("Interface size")}.querySelector('select')`, "125"));
  await page.waitFor(`Math.abs(window.devicePixelRatio / ${before} - 1.25) < 0.01`);
  if (shots) await page.screenshot(join(shots, "theme-settings.png"));
  await page.eval(change(`${setting("Interface size")}.querySelector('select')`, "100"));
  await page.waitFor(`Math.abs(window.devicePixelRatio - ${before}) < 0.01`);

  // Reset puts the theme's accent back.
  await page.eval(`[...${setting("Accent color")}.querySelectorAll('button')].find(b => b.textContent === 'Reset').click()`);
  await page.waitFor(`${cssVar("--accent")} === '#88c0d0'`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);

  // The sidebar button leaves a preset for the system theme, then Platypus Light and Dark.
  const toggle = `document.querySelector('button[title^="Theme:"]')`;
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`${toggle}.title.startsWith('Theme: System')`);
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`${toggle}.title.startsWith('Theme: Platypus Light')`);
  await page.waitFor(`${cssVar("--bg")} === '#ffffff'`);
  await saved(dir, /theme = "platypus_light"/);
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`${toggle}.title.startsWith('Theme: Platypus Dark')`);
  await page.waitFor(`${cssVar("--bg")} === '#141519'`);
}
