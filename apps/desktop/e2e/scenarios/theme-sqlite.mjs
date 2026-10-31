// The Appearance settings: a theme preset, a custom accent color, grid density and interface
// size apply at once and are saved; the sidebar button cycles system, Platypus Light and Dark.
// The High contrast theme: black on white-on-black variables, the contrast ratios of its text,
// borders, accent, status colors and syntax colors on the surfaces they sit on (WCAG AAA for
// text), kept by a custom accent and saved.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

const cssVar = (name) => `getComputedStyle(document.documentElement).getPropertyValue(${JSON.stringify(name)}).trim()`;
/** A color variable as the browser resolves it (color-mix included), as [r, g, b]. */
const rgbOf = (name) => `(() => {
  const probe = document.createElement('div');
  probe.style.color = 'var(${name})';
  document.body.append(probe);
  const c = getComputedStyle(probe).color;
  probe.remove();
  // rgb(r, g, b) with 0-255, or color(srgb r g b) with 0-1 for color-mix results.
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
  if (shots) {
    await page.waitFor(`document.querySelector('.empty-import') !== null`);
    await page.screenshot(join(shots, "navigator-empty.png"));
  }

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
  const rgb = async (name) => JSON.parse(await page.eval(rgbOf(name)));
  assert.ok(["#16191b", "#ffffff"].includes(await css("--bg")), await css("--bg"));

  // Every preset sets the variables added with the Platypus themes.
  const pick = (label) =>
    page.eval(`[...document.querySelectorAll('.theme-card')].find(c => c.textContent === ${JSON.stringify(label)}).click()`);
  const NEW_VARS = ["--bg-rail", "--bg-tabbar", "--fg-strong", "--border-strong", "--active",
    "--obj-db", "--obj-table", "--obj-view", "--obj-routine", "--obj-trigger",
    "--type-dev", "--type-dev-bg", "--type-test", "--type-test-bg", "--type-prod", "--type-prod-bg",
    "--warn", "--frame-dev", "--frame-test", "--frame-prod"];
  const labels = JSON.parse(await page.eval(`JSON.stringify([...document.querySelectorAll('.theme-card')].map(c => c.textContent))`));
  assert.ok(labels.includes("Platypus Dark") && labels.includes("Classic Dark"), labels.join(", "));
  for (const label of labels) {
    await pick(label);
    await page.waitFor(`document.querySelector('.theme-card.selected')?.textContent === ${JSON.stringify(label)}`);
    for (const v of NEW_VARS) assert.notEqual(await css(v), "", `${label} ${v}`);
  }

  await pick("Platypus Dark");
  await page.waitFor(`${cssVar("--bg")} === '#16191b'`);
  assert.deepEqual(
    [await css("--bg-sidebar"), await css("--bg-rail"), await css("--border"), await css("--accent")],
    ["#1b1f22", "#111416", "#262c30", "#ff8d28"],
  );
  await pick("Classic Dark");
  await page.waitFor(`${cssVar("--bg")} === '#1b1d21'`); // the former "Dark"

  // A custom background on Platypus Dark: the tab bar follows it; the rail and the active row
  // go with the sidebar and the accent, so they keep the theme's colors.
  await pick("Platypus Dark");
  const background = async (hex) => {
    await page.eval(change(`${setting("Background")}.querySelector('input[type=color]')`, hex));
    await page.waitFor(`${cssVar("--bg")} === '${hex}'`);
  };
  await background("#202020");
  const tabbar = await rgb("--bg-tabbar");
  await background("#303030");
  assert.notEqual(await rgb("--bg-tabbar"), tabbar);
  assert.equal(await css("--bg-rail"), "#111416");
  assert.equal(await css("--active"), "#20262a");

  // A light custom background on a dark theme: what is derived from it goes the light way.
  await background("#ffffff");
  await page.eval(change(`${setting("Text")}.querySelector('input[type=color]')`, "#222222"));
  await page.waitFor(`${cssVar("--fg")} === '#222222'`);
  const bar = await rgb("--bg-tabbar");
  assert.ok(luminance(bar) > 0.6, `tab bar stays light: ${JSON.stringify(bar)} ${await css("--bg-tabbar")}`);
  assert.ok(
    contrast(await rgb("--fg-strong"), await rgb("--bg")) >= contrast(await rgb("--fg"), await rgb("--bg")),
    "strong text at least as strong as text",
  );
  for (const label of ["Background", "Text"]) {
    await page.eval(`[...${setting(label)}.querySelectorAll('button')].find(b => b.textContent === 'Reset').click()`);
  }
  await page.waitFor(`${cssVar("--bg")} === '#16191b'`);

  // Platypus Light: secondary text readable on every surface it sits on.
  await pick("Platypus Light");
  await page.waitFor(`${cssVar("--bg")} === '#ffffff'`);
  for (const surface of ["--bg", "--bg-sidebar", "--bg-tabbar", "--grid-header"]) {
    const ratio = contrast(await rgb("--fg-muted"), await rgb(surface));
    assert.ok(ratio >= 4.5, `--fg-muted on ${surface}: ${ratio.toFixed(2)}`);
  }
  await pick("Platypus Dark");
  await page.waitFor(`${cssVar("--bg")} === '#16191b'`);

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

  // The app draws the title bar in its top bar, with the window buttons; "System" gives it back.
  assert.equal(await page.eval(`document.querySelectorAll('.window-controls button').length`), 3);
  await page.settingsTab("Appearance", "Window");
  await page.eval(change(`${setting("Title bar")}.querySelector('select')`, "system"));
  await page.waitFor(`document.querySelector('.window-controls') === null`);
  assert.equal(await page.eval(`document.querySelector('.topbar').hasAttribute('data-tauri-drag-region')`), false);
  await page.eval(change(`${setting("Title bar")}.querySelector('select')`, "app"));
  await page.waitFor(`document.querySelectorAll('.window-controls button').length === 3`);

  await page.settingsTab("Appearance", "Density");
  await page.eval(change(`${setting("Grid rows")}.querySelector('select')`, "compact"));
  await page.waitFor(`${cssVar("--grid-row-h")} === '20'`);

  const before = await page.eval(`window.devicePixelRatio`);
  await page.eval(change(`${setting("Interface size")}.querySelector('select')`, "125"));
  await page.waitFor(`Math.abs(window.devicePixelRatio / ${before} - 1.25) < 0.01`);
  if (shots) await page.screenshot(join(shots, "theme-settings.png"));
  await page.eval(change(`${setting("Interface size")}.querySelector('select')`, "100"));
  await page.waitFor(`Math.abs(window.devicePixelRatio - ${before}) < 0.01`);

  // Reset puts the theme's accent back.
  await page.settingsTab("Appearance", "Theme");
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
  await page.waitFor(`${cssVar("--bg")} === '#16191b'`);

  await highContrast(page, dir);
}

/** The High contrast preset: its variables, and the contrast between them. */
async function highContrast(page, dir) {
  const css = (name) => page.eval(cssVar(name));
  const rgb = async (name) => JSON.parse(await page.eval(rgbOf(name)));
  const ratio = async (a, b) => contrast(await rgb(a), await rgb(b));
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance", "Theme");
  await page.eval(`[...document.querySelectorAll('.theme-card')].find(c => c.textContent === 'High contrast').click()`);
  await page.waitFor(`${cssVar("--bg")} === '#000000'`);
  await saved(dir, /theme = "high_contrast"/);
  assert.equal(await css("--fg"), "#ffffff");
  assert.equal(await css("--accent"), "#ffd400");
  assert.equal(await page.eval(`document.documentElement.style.colorScheme`), "dark");

  // Text: AAA (7:1) for the main and the secondary text on every surface they sit on.
  for (const surface of ["--bg", "--bg-sidebar", "--bg-input", "--grid-bg", "--grid-bg-alt", "--grid-header"]) {
    assert.ok((await ratio("--fg", surface)) >= 7, `--fg on ${surface}`);
    assert.ok((await ratio("--fg-muted", surface)) >= 7, `--fg-muted on ${surface}: ${(await ratio("--fg-muted", surface)).toFixed(2)}`);
  }
  // Borders are seen (3:1 for interface components), the accent reads as text, the accent's own
  // text reads on it, and the status colors read on their backgrounds.
  assert.ok((await ratio("--border", "--bg")) >= 3, "border");
  assert.ok((await ratio("--accent", "--bg")) >= 7, "accent on the background");
  assert.ok((await ratio("--accent-fg", "--accent")) >= 7, "text on the accent");
  assert.ok((await ratio("--error", "--error-bg")) >= 7, "error");
  assert.ok((await ratio("--ok", "--ok-bg")) >= 7, "ok");
  // The code on the grid and editor background.
  for (const v of ["--syn-keyword", "--syn-string", "--syn-number", "--syn-comment", "--syn-type", "--syn-ident"]) {
    assert.ok((await ratio(v, "--grid-bg")) >= 7, v);
  }

  // A custom accent on it still reads, with the right text color on it; Reset gives yellow back.
  await page.eval(change(`${setting("Accent color")}.querySelector('input[type=color]')`, "#0044ff"));
  await page.waitFor(`${cssVar("--accent")} === '#0044ff'`);
  assert.ok((await ratio("--accent-fg", "--accent")) >= 4.5, "text on a custom accent");
  await page.eval(`[...${setting("Accent color")}.querySelectorAll('button')].find(b => b.textContent === 'Reset').click()`);
  await page.waitFor(`${cssVar("--accent")} === '#ffd400'`);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
}

