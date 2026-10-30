// Settings › Appearance › Icons and animations: the animations as the system asks, on,
// reduced or off, each an attribute on the root that the styles follow (dialogs rise, menus
// drop, the sub-tab's underline grows; reduced only fades; off neither), saved and reset; the
// icon families as cards, the app's icons following the one chosen. The dialog fits a
// 1366×768 window.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { settingReset as resetOf, settingRow as setting, settingsFile, settingsLack, settingsSubTab as subTab } from "../settings.mjs";

const animations = `${setting("appearance.animations")}.querySelector('select')`;
const card = (value) => `${setting("appearance.icon_set")}?.querySelector('[role=radio][data-value="${value}"]')`;
const motion = `document.documentElement.dataset.motion`;
const systemMotion = `(matchMedia('(prefers-reduced-motion: reduce)').matches ? 'reduced' : 'full')`;
const style = (expr, prop, pseudo = null) => `getComputedStyle(${expr}${pseudo ? `, ${JSON.stringify(pseudo)}` : ""}).${prop}`;
/** A context menu's styles, on one made for the check and taken away. */
const menuStyle = (prop) => `(() => {
  const menu = document.createElement('div');
  menu.className = 'context-menu';
  document.body.append(menu);
  const value = getComputedStyle(menu).${prop};
  menu.remove();
  return value;
})()`;
const appearanceIcon = `document.querySelector('.modal.settings [role=tab][data-section="Appearance"] svg').outerHTML`;

/** Chooses `value` in a select as the user would, for React's `onChange`. */
const choose = (element, value) => `(() => {
  const el = ${element};
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

export const ready = `document.querySelector('.shell') !== null`;

async function openSubTab(page, label) {
  await page.waitFor(`${subTab(label)} !== null`);
  await page.eval(`${subTab(label)}.click()`);
  await page.waitFor(`${subTab(label)}.getAttribute('aria-selected') === 'true'`);
}

export default async function (page, { dir, shots }) {
  // Like the system by default.
  await page.waitFor(`${motion} === ${systemMotion}`);

  // In a 1366×768 window the dialog fits, with the icon families side by side.
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1366, height: 768, deviceScaleFactor: 1, mobile: false });
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Appearance");
  await openSubTab(page, "Icons and animations");
  await page.waitFor(`${setting("appearance.icon_set")}?.offsetParent != null`);
  const fits = `(() => { const r = document.querySelector('.modal.settings').getBoundingClientRect(); return r.top >= 0 && r.bottom <= innerHeight && r.right <= innerWidth; })()`;
  await page.waitFor(fits);
  const tops = await page.eval(
    `["lucide", "tabler", "phosphor"].map(v => Math.round(document.querySelector('.modal.settings [data-key="appearance.icon_set"] [data-value="' + v + '"]').getBoundingClientRect().top))`,
  );
  assert.equal(new Set(tops).size, 1, `the three cards in a row: ${tops}`);
  // Each card shows the same five icons, drawn by its family.
  for (const v of ["lucide", "tabler", "phosphor"]) {
    assert.equal(await page.eval(`${card(v)}.querySelectorAll('.picture-icons svg').length`), 5, v);
  }
  const samples = await page.eval(`["lucide", "tabler", "phosphor"].map(v => document.querySelector('.modal.settings [data-value="' + v + '"] .picture-icons').innerHTML)`);
  assert.equal(new Set(samples).size, 3, "three different families");
  assert.equal(await page.eval(`${card("lucide")}.getAttribute('aria-checked')`), "true");
  if (shots) await page.screenshot(join(shots, "motion-1366.png"));
  await openSubTab(page, "Layout");
  await page.waitFor(fits);
  await openSubTab(page, "Icons and animations");

  // The four choices of the animations.
  assert.deepEqual(await page.eval(`[...${animations}.options].map(o => o.value)`), ["system", "on", "reduced", "off"]);
  assert.equal(await page.eval(`${animations}.value`), "system");

  // On: dialogs rise, menus drop, the chosen sub-tab's underline grows in.
  await page.eval(choose(animations, "on"));
  await page.waitFor(`${motion} === 'full'`);
  await settingsFile(dir, /\[appearance\][^[]*animations = "on"/);
  assert.equal(await page.eval(style(`document.querySelector('.modal.settings')`, "animationName")), "motion-rise");
  assert.equal(await page.eval(menuStyle("animationName")), "motion-drop");
  const chosen = `document.querySelector('.modal.settings .dialog-tabs [aria-selected="true"]')`;
  const other = `document.querySelector('.modal.settings .dialog-tabs [aria-selected="false"]')`;
  assert.match(await page.eval(style(chosen, "transitionProperty", "::after")), /transform/);
  // The underline fades in: wait for the end of the transition.
  await page.waitFor(`${style(chosen, "opacity", "::after")} === "1" && ${style(other, "opacity", "::after")} === "0"`);
  // Short: none over 200 ms.
  const ms = (s) => parseFloat(s) * (s.endsWith("ms") ? 1 : 1000);
  assert.ok(ms(await page.eval(style(`document.querySelector('.modal.settings')`, "animationDuration"))) <= 200);
  assert.ok(ms(await page.eval(menuStyle("animationDuration"))) <= 200);

  // Reduced: the same moments fade, nothing moves.
  await page.eval(choose(animations, "reduced"));
  await page.waitFor(`${motion} === 'reduced'`);
  await settingsFile(dir, /animations = "reduced"/);
  assert.equal(await page.eval(style(`document.querySelector('.modal.settings')`, "animationName")), "motion-fade");
  assert.equal(await page.eval(menuStyle("animationName")), "motion-fade");
  assert.equal(await page.eval(style(other, "transform", "::after")), "none");
  assert.doesNotMatch(await page.eval(style(`document.querySelector('.modal.settings .actions button')`, "transitionProperty")), /transform/);

  // Off: no animation and no transition.
  await page.eval(choose(animations, "off"));
  await page.waitFor(`${motion} === 'none'`);
  await settingsFile(dir, /animations = "off"/);
  assert.equal(await page.eval(style(`document.querySelector('.modal.settings')`, "animationDuration")), "0s");
  assert.equal(await page.eval(style(`document.querySelector('.modal.settings .actions button')`, "transitionDuration")), "0s");
  assert.equal(await page.eval(menuStyle("animationDuration")), "0s");

  // Reset: like the system again, following its "reduce motion" as it changes.
  await page.eval(`${resetOf("appearance.animations")}.click()`);
  await page.waitFor(`${motion} === ${systemMotion}`);
  await settingsLack(dir, /animations/);
  await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] });
  await page.waitFor(`${motion} === 'reduced'`);
  await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "no-preference" }] });
  await page.waitFor(`${motion} === 'full'`);
  await page.send("Emulation.setEmulatedMedia", { features: [] });

  // The icon family: chosen from its card, the app's icons change; Reset goes back to Lucide.
  const lucide = await page.eval(appearanceIcon);
  await page.eval(`${card("tabler")}.click()`);
  await settingsFile(dir, /icon_set = "tabler"/);
  await page.waitFor(`${card("tabler")}.getAttribute('aria-checked') === 'true' && ${appearanceIcon} !== ${JSON.stringify(lucide)}`);
  await page.eval(`${card("phosphor")}.click()`);
  await settingsFile(dir, /icon_set = "phosphor"/);
  await page.waitFor(`${card("phosphor")}.getAttribute('aria-checked') === 'true'`);
  if (shots) await page.screenshot(join(shots, "motion-phosphor.png"));
  await page.eval(`${resetOf("appearance.icon_set")}.click()`);
  await settingsLack(dir, /icon_set/);
  await page.waitFor(`${card("lucide")}.getAttribute('aria-checked') === 'true' && ${appearanceIcon} === ${JSON.stringify(lucide)}`);

  // The search finds them by name.
  const search = `document.querySelector('.modal.settings .modal-search input')`;
  await page.eval(`(() => {
    const el = ${search};
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, 'animations');
    el.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  await page.waitFor(`${setting("appearance.animations")} !== null`);

  await page.send("Emulation.clearDeviceMetricsOverride", {});
}
