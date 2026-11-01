// Settings dialog, Sync page and shortcuts (group T, P2): a number out of range stays marked with a
// toast saying what is accepted; a text field saves on Enter and when Esc closes the dialog; the
// changed dot explains itself; the shortcut page has its instruction line and keys on the right;
// the Themes page no longer promises a choice; the Sync page lays out like the others (spaced
// groups, full-width folder field, settings rows).
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { settingRow, settingsFile } from "../settings.mjs";

const modal = `document.querySelector('.modal.settings')`;
const input = (key) => `${settingRow(key)}.querySelector('input')`;

/** Types into an input as the user would, for React's `onChange`/`input`, and focuses it. */
const typeInto = (expr, value) => `(() => {
  const el = ${expr};
  el.focus();
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('input', { bubbles: true }));
})()`;

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page, { dir }) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`${modal} !== null`);

  // A big window gives a bigger dialog, and then the last group of pages is in view.
  const { vw, vh, w, h } = await page.eval(`(() => { const r = ${modal}.getBoundingClientRect(); return { vw: innerWidth, vh: innerHeight, w: r.width, h: r.height }; })()`);
  if (vw >= 1500) assert.ok(w > 960, `the dialog grows on a wide window: ${w}`);
  if (vh >= 1000) {
    const fits = await page.eval(`(() => { const n = document.querySelector('.settings-nav'); return n.scrollHeight <= n.clientHeight + 1; })()`);
    assert.ok(fits, "every page of the list is in view without scrolling");
  }
  assert.ok(h > 0);

  // Navigator: LIMIT has a description; a number below the minimum stays in the field, marked,
  // with a toast; a valid one clears the mark.
  await page.settingsTab("Navigator");
  const key = "navigator.select_limit";
  await page.waitFor(`${settingRow(key)} !== null`);
  assert.ok((await page.eval(`${settingRow(key)}.textContent`)).includes("Rows of the SELECT"), "the LIMIT setting is described");
  await page.eval(typeInto(input(key), "0"));
  await page.eval(`${input(key)}.blur()`);
  await page.waitFor(`${input(key)}.getAttribute('aria-invalid') === 'true'`);
  await page.waitFor(`document.querySelector('.toast.error')?.textContent.includes('At least 1')`);
  assert.equal(await page.eval(`${input(key)}.value`), "0", "the rejected value stays in the field");
  await page.eval(typeInto(input(key), "250"));
  await page.waitFor(`${input(key)}.getAttribute('aria-invalid') === null`);
  await page.eval(`${input(key)}.blur()`);
  await settingsFile(dir, /select_limit\s*=\s*250/);
  // The changed dot has a tooltip.
  await page.waitFor(`${settingRow(key)}.querySelector('.setting-title.changed')?.title === 'Different from the default'`);
  // The first setting of a page without sub-tabs does not touch the heading.
  const gap = await page.eval(`${settingRow(key)}.getBoundingClientRect().top - document.querySelector('.settings-head').getBoundingClientRect().bottom`);
  assert.ok(gap >= 8, `space under the page head: ${gap}`);

  // Safety › Masking: Enter saves a text field; Esc with the field still focused saves it too.
  await page.settingsTab("Safety", "Masking");
  const types = "safety.mask_types";
  await page.waitFor(`${input(types)}?.offsetParent != null`);
  await page.eval(typeInto(input(types), "inet"));
  await page.key("Enter", "Enter", 13);
  await settingsFile(dir, /mask_types\s*=\s*"inet"/);
  await page.eval(typeInto(input(types), "inet, bytea"));
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${modal} === null`);
  await settingsFile(dir, /mask_types\s*=\s*"inet, bytea"/);

  // Themes: the subtitle no longer says "choose"; the shortcut page explains how to assign a key.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`${modal} !== null`);
  await page.settingsTab("Themes");
  await page.waitFor(`document.querySelector('.settings-head p')?.textContent === 'Import and export the color themes.'`);
  await page.settingsTab("Keyboard shortcuts");
  await page.waitFor(`document.querySelector('.shortcuts > p.muted')?.textContent.includes('Click a shortcut')`);
  const align = await page.eval(`(() => {
    const cell = document.querySelector('.shortcuts tbody tr:not(.shortcuts-scope) td:nth-child(2)');
    const row = cell.parentElement.getBoundingClientRect();
    const key = cell.querySelector('.shortcut-key').getBoundingClientRect();
    return { gapRight: cell.getBoundingClientRect().right - key.right, row: row.width };
  })()`);
  assert.ok(align.gapRight < 20, `the key sits at the right of its column: ${align.gapRight}`);

  // Sync: spaced groups, the folder field as wide as the page, settings as rows.
  await page.settingsTab("Sync");
  await page.waitFor(`document.querySelector('.sync-panel') !== null`);
  const lay = await page.eval(`(() => {
    const p = document.querySelector('.sync-panel').getBoundingClientRect();
    const f = document.querySelector('[data-field=sync-folder]').getBoundingClientRect();
    const fields = [...document.querySelectorAll('.sync-panel > label')].map((l) => l.getBoundingClientRect());
    return { panel: p.width, folder: f.width, gaps: fields.slice(1).map((r, i) => r.top - fields[i].bottom), rows: document.querySelectorAll('.sync-panel > label.setting').length };
  })()`);
  assert.ok(lay.folder > lay.panel * 0.6, `the folder field is wide: ${lay.folder} of ${lay.panel}`);
  assert.ok(lay.gaps.every((g) => g >= 8), `the groups are spaced: ${lay.gaps}`);
  assert.equal(lay.rows, 2, "interval and start-up are setting rows");
}
