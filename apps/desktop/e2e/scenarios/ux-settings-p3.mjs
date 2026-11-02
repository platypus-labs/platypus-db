// Settings pages (group T, P3): one shared space under the page head, a real empty state for
// the plugins, the diagnostics text split in two with a Copy button, the state of the master
// password as a title, titled parts in Drivers, a textarea for the sensitive column names, the
// animation choices as On / Reduced / Off, short Toolbar rows, and the Sync buttons together
// under the last-sync line.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { settingRow } from "../settings.mjs";

const modal = `document.querySelector('.modal.settings')`;

/** Pixels between the page head and the first thing under it. */
const headGap = `(() => {
  const head = document.querySelector('.settings-head');
  return head.nextElementSibling.getBoundingClientRect().top - head.getBoundingClientRect().bottom;
})()`;

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`${modal} !== null`);

  // The space under the head is the same on every page that starts with text or buttons.
  for (const [name, root] of [
    ["Themes", ".themes-panel"],
    ["Master password", ".master-password"],
    ["Plugins", ".plugins-panel"],
    ["Diagnostics", ".diagnostics-panel"],
    ["Sync", ".sync-panel"],
    ["Drivers", ".drivers-panel"],
    ["Agents", ".agents-panel"],
  ]) {
    await page.settingsTab(name);
    await page.waitFor(`document.querySelector('${root}') !== null`);
    const gap = await page.eval(headGap);
    assert.ok(gap >= 16, `${name}: space under the page head: ${gap}`);
  }

  // Plugins: nothing installed is a state with both install buttons, not a line of text.
  await page.settingsTab("Plugins");
  await page.waitFor(`document.querySelector('.plugins-panel .state.empty [data-action="install-folder"]') !== null`);
  assert.ok(await page.eval(`document.querySelector('.plugins-panel .state.empty [data-action="install-zip"]') !== null`));
  assert.match(await page.eval(`document.querySelector('.plugins-panel .state.empty').textContent`), /No plugin installed\./);

  // Diagnostics: two paragraphs, the log folder with a Copy button.
  await page.settingsTab("Diagnostics");
  await page.waitFor(`document.querySelectorAll('.diagnostics-panel > p.muted').length === 2`);
  await page.waitFor(`[...document.querySelectorAll('.diagnostics-logs button')].some((b) => b.textContent === 'Copy')`);

  // Master password: its state is a title.
  await page.settingsTab("Master password");
  await page.waitFor(`document.querySelector('[data-field="vault-state"]')?.textContent === 'The master password is off'`);

  // Drivers: titled parts.
  await page.settingsTab("Drivers");
  await page.waitFor(`[...document.querySelectorAll('.drivers-panel .settings-subhead')].map((h) => h.textContent).join('|') === 'Downloaded drivers|Client drivers'`);

  // Sync: the last-sync line is above the buttons, which sit together ("Turn off" next to the others).
  await page.settingsTab("Sync");
  const sync = await page.eval(`(() => {
    const last = document.querySelector('[data-field="sync-last"]').getBoundingClientRect();
    const now = document.querySelector('[data-action="sync-now"]').getBoundingClientRect();
    const off = document.querySelector('[data-action="sync-off"]').getBoundingClientRect();
    return { lastAbove: last.bottom <= now.top, apart: off.left - now.right };
  })()`);
  assert.ok(sync.lastAbove, "the last sync is above the buttons");
  assert.ok(sync.apart < 24, `Turn off next to Sync now: ${sync.apart}`);

  // Safety › Masking: the names are a textarea as wide as the page; the types have a hint.
  await page.settingsTab("Safety", "Masking");
  const names = `${settingRow("safety.mask_names")}.querySelector('textarea')`;
  await page.waitFor(`${names}?.offsetParent != null`);
  const wide = await page.eval(`${names}.getBoundingClientRect().width > 400`);
  assert.ok(wide, "the list of column names has room");
  assert.equal(await page.eval(`${settingRow("safety.mask_types")}.querySelector('input').placeholder`), "inet, bytea");

  // Appearance › animations: On / Reduced / Off.
  await page.settingsTab("Appearance", "Icons and animations");
  const options = `${settingRow("appearance.animations")}.querySelector('select')`;
  await page.waitFor(`${options}?.offsetParent != null`);
  assert.deepEqual(await page.eval(`[...${options}.options].map((o) => o.textContent)`), ["Like the system", "Enabled", "Reduced", "Off"]);

  // Toolbar: the checkbox rows are short.
  await page.settingsTab("Toolbar");
  await page.waitFor(`document.querySelector('.settings-body .setting') !== null`);
  const rows = await page.eval(`(() => {
    const box = [...document.querySelectorAll('.settings-body .setting')].find((r) => r.querySelector('input[type=checkbox]'));
    return box.getBoundingClientRect().height;
  })()`);
  assert.ok(rows <= 46, `a checkbox row is ${rows}px`);
}
