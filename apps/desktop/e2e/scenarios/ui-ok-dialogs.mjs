// QA ui (works, regression guard): the shell's dialogs carry the dialog role and a name, take
// the focus when they open, keep Tab inside and close with Escape; the Settings' search
// finds settings by words of their label, shows "No settings match." and Escape first clears
// it, then closes; every toolbar setting hides its buttons at once.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { sqliteConnection, treeRow, visibleTab } from "../query.mjs";

const NAME = "QA UI dialogs";
const setting = (label) =>
  `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_ui_dialogs.db",
    id: "00000000-0000-4000-8000-00000000e132",
    name: NAME,
    sql: "CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY);",
  });
}

async function probe(page, open, selector, name) {
  await page.eval(open);
  await page.waitFor(`document.querySelector(${JSON.stringify(selector)}) !== null`);
  await page.waitFor(`document.querySelector(${JSON.stringify(selector)}).contains(document.activeElement)`);
  const info = await page.eval(`(() => { const m = document.querySelector(${JSON.stringify(selector)}); return { role: m.getAttribute('role'), name: document.getElementById(m.getAttribute('aria-labelledby'))?.textContent ?? m.getAttribute('aria-label') }; })()`);
  assert.equal(info.role, "dialog", name);
  assert.equal(info.name, name);
  // Tab cycles inside.
  for (let i = 0; i < 25; i++) await page.key("Tab", "Tab", 9);
  assert.equal(await page.eval(`document.querySelector(${JSON.stringify(selector)}).contains(document.activeElement)`), true, `${name}: Tab left the dialog`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector(${JSON.stringify(selector)}) === null`);
}

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await probe(page, `document.querySelector('.sidebar-column button[aria-label="New connection"]').click()`, ".modal", "New connection");
  await probe(page, `document.querySelector('.sidebar-column button[aria-label="Import connections"]').click()`, ".modal", "Import connections and projects");
  await probe(page, `document.querySelector('.activity-button').click()`, ".modal.activity", "Activity");
  await probe(page, `document.querySelector('.project-add').click()`, ".project-dialog", "New project");

  // Settings search.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings input') === document.activeElement`);
  await page.type("font");
  await page.waitFor(`document.querySelectorAll('.settings-results section:not([hidden]) .setting').length === 2`);
  assert.deepEqual(
    await page.eval(`[...document.querySelectorAll('.settings-results section:not([hidden]) .setting')].map(s => s.querySelector('span')?.firstChild?.textContent)`),
    ["SQL editor font size", "Result grid font size"],
  );
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("zzqq");
  await page.waitFor(`document.querySelector('.settings-none')?.textContent === 'No settings match.'`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings input').value === ''`);
  assert.equal(await page.eval(`document.querySelector('.modal.settings') !== null`), true);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);

  // Toolbar settings.
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.toolbar') != null`);
  const labels = `[...${visibleTab}.querySelectorAll('.toolbar button')].filter(b => b.getClientRects().length).map(b => b.getAttribute('aria-label') || b.textContent.trim())`;
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Toolbar");
  const hides = {
    "Run script button": ["Script"],
    "Cancel button": ["Cancel"],
    "Auto-commit, Commit and Rollback": ["Auto-commit"],
    "Open and Save script": ["Open", "Save"],
    History: ["History"],
    "Variables, Outline and Terminal": ["Terminal", "Variables", "Outline"],
  };
  for (const [label, buttons] of Object.entries(hides)) {
    await page.eval(`${setting(label)}.querySelector('input').click()`);
    await page.waitFor(`!${labels}.includes(${JSON.stringify(buttons[0])})`);
    const now = await page.eval(labels);
    for (const b of buttons) assert.ok(!now.includes(b), `${label}: ${b} still shown`);
    assert.ok(now.includes("Run"), `${label}: Run hidden`);
    await page.eval(`${setting(label)}.querySelector('input').click()`);
    await page.waitFor(`${labels}.includes(${JSON.stringify(buttons[0])})`);
  }
}
