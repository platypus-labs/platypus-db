// QA ui (works, regression guard): the theme "System" follows the system's light or dark
// mode while the app runs; Ctrl+Tab and Ctrl+PageUp/PageDown cycle the tabs and wrap; with
// many tabs the active one stays in view; Close all keeps the pinned tab; the zoom keys
// stop at 80% and 150% and the keypad's + works; the Settings reopen on the last section.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow } from "../query.mjs";
import { settingsFile } from "../settings.mjs";

const NAME = "QA UI keys";
const tabs = `[...document.querySelectorAll('.tab')]`;
const active = `${tabs}.findIndex(t => t.classList.contains('active'))`;
const inView = `(() => { const bar = document.querySelector('.tabs').getBoundingClientRect(); const t = document.querySelector('.tab.active').getBoundingClientRect(); return t.left >= bar.left - 1 && t.right <= bar.right + 1; })()`;
const bg = `getComputedStyle(document.documentElement).getPropertyValue('--bg').trim()`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_keys.db",
    id: "00000000-0000-4000-8000-00000000e131",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

export default async function (page, { dir }) {
  // System theme (the default) follows the system.
  await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "light" }] });
  await page.waitFor(`${bg} === '#ffffff'`);
  await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
  await page.waitFor(`${bg} === '#16191b'`);
  await page.send("Emulation.setEmulatedMedia", { features: [] });

  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${tabs}.length === 1`);
  for (let n = 2; n <= 20; n++) {
    await page.key("t", "KeyT", 84, CTRL);
    await page.waitFor(`${tabs}.length === ${n}`);
  }
  await page.waitFor(`document.querySelector('.tabs').scrollWidth > document.querySelector('.tabs').clientWidth`);
  await page.waitFor(inView);
  assert.equal(await page.eval(active), 19);
  await page.key("Tab", "Tab", 9, CTRL);
  await page.waitFor(`${active} === 0`);
  await page.waitFor(inView);
  await page.key("PageUp", "PageUp", 33, CTRL);
  await page.waitFor(`${active} === 19`);
  await page.key("PageDown", "PageDown", 34, CTRL);
  await page.waitFor(`${active} === 0`);

  // Close all keeps the pinned tab.
  await page.eval(`${tabs}[5].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 300, clientY: 60 }))`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu [role=menuitem]')].find(b => b.textContent.startsWith('Pin tab')).click()`);
  await page.waitFor(`${tabs}[0].classList.contains('pinned')`);
  await page.eval(`${tabs}[3].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 300, clientY: 60 }))`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu [role=menuitem]')].find(b => b.textContent.startsWith('Close all')).click()`);
  await page.waitFor(`${tabs}.length === 1`);
  assert.equal(await page.eval(`${tabs}[0].classList.contains('pinned')`), true);

  // Zoom bounds.
  for (let i = 0; i < 5; i++) await page.key("-", "Minus", 189, CTRL);
  await settingsFile(dir, /zoom = "80"/);
  for (let i = 0; i < 8; i++) await page.key("=", "Equal", 187, CTRL);
  await settingsFile(dir, /zoom = "150"/);
  await page.key("0", "Digit0", 48, CTRL);
  await page.waitFor(`Math.abs(devicePixelRatio - 1) < 0.01`);
  await page.key("+", "NumpadAdd", 107, CTRL);
  await settingsFile(dir, /zoom = "110"/);
  await page.key("0", "Digit0", 48, CTRL);

  // The Settings open again on the section shown last.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Toolbar");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings [role=tab][aria-selected=true]')?.dataset.section === 'Toolbar'`);
}
