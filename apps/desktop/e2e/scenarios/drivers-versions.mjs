// Updates and versions of a downloaded driver, against a fake registry served locally
// (PLATYPUS_DRIVER_REGISTRY): the profile starts with Trino v1.0.0 installed and the registry
// has v1.0.0, v1.1.0, v1.2.0 and a v0.9.0 whose hash does not match. The startup check shows the
// "update available" badge; Update installs v1.2.0 and keeps v1.0.0; the compatibility check
// connects to the saved connection (the fake library cannot be loaded, so it fails), leaves the
// one that asks for its password, and offers to go back; Versions… installs and activates earlier
// versions and deletes installed ones. The two settings are in Settings › Driver updates.
// No network, no database: the libraries are not real, so a passing probe is not covered.
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { settingsFile, settingsLack } from "../settings.mjs";
import { ASKS, SAVED, badge, button, compat, fakeRegistry, row, seedDriver, trinoConnections, versionOf, versionRow } from "../drivers-registry.mjs";

let registry = "";

export async function setup({ dir }) {
  registry = await fakeRegistry({ versions: ["v1.0.0", "v1.1.0", "v1.2.0", "v0.9.0"], corrupted: ["v0.9.0"] });
  seedDriver(dir, "v1.0.0");
  trinoConnections(dir);
}

export const env = () => ({ PLATYPUS_DRIVER_REGISTRY: registry });

const panelError = `document.querySelector('.modal.settings .drivers-panel .notice.error')`;
const setting = (label) =>
  `[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('span')?.firstChild?.textContent === ${JSON.stringify(label)})`;
const states = `[...${compat}.querySelectorAll('.multi-run-row')].map(r => r.dataset.state)`;
const versionStates = `[...${row}.querySelectorAll('.driver-version')].map(v => v.dataset.version + ':' + v.dataset.state)`;

/** Waits for the compatibility dialog to finish its probes. */
async function finishedCheck(page) {
  await page.waitFor(`${compat} != null && ${compat}.querySelector('[data-field="summary"]') != null`, 20_000);
}

/** Closes the finished compatibility dialog. */
async function closeCheck(page) {
  await page.eval(`${compat}.querySelector('[data-action="close"]').click()`);
  await page.waitFor(`${compat} === null`);
}

export default async function (page, { dir }) {
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`document.querySelector('button[aria-label="Settings"]').click()`);

  // The two settings: the check is on by default, the automatic install is off.
  await page.settingsTab("Driver updates");
  await page.waitFor(`${setting("Look for driver updates at startup")} != null`);
  assert.equal(await page.eval(`${setting("Look for driver updates at startup")}.querySelector('input').checked`), true);
  assert.equal(await page.eval(`${setting("Install driver updates by itself")}.querySelector('input').checked`), false);
  await page.eval(`${setting("Install driver updates by itself")}.querySelector('input').click()`);
  await settingsFile(dir, /auto_update = true/);
  await page.eval(`${setting("Install driver updates by itself")}.querySelector('input').click()`);
  await settingsLack(dir, /auto_update/);

  // The startup check (nothing was clicked) found v1.2.0, and noted when it asked.
  await page.settingsTab("Drivers");
  await page.waitFor(`${badge("v1.2.0")} != null`, 20_000);
  assert.equal(await page.eval(versionOf), "v1.0.0");
  assert.ok(existsSync(join(dir, "drivers", "last-update-check")));
  const native = await page.eval(`document.querySelector('.drivers-panel [data-field="native-drivers"]').textContent`);
  assert.match(native, /built into PlatypusDB( \d+\.\d+\.\d+)? and update with the app/);

  // Update: v1.2.0 is installed and active, v1.0.0 stays. The check tries the connection that
  // keeps its password (it fails: the library is fake), leaves the other, and offers the way back.
  await page.eval(`${button(row, "Update")}.click()`);
  await finishedCheck(page);
  assert.deepEqual(await page.eval(states), ["failed", "skipped"]);
  const summary = await page.eval(`${compat}.querySelector('[data-field="summary"]').textContent`);
  assert.ok(summary.includes("Working: 0") && summary.includes("Failed: 1") && summary.includes("Not tried: 1"), summary);
  const names = await page.eval(`[...${compat}.querySelectorAll('.multi-run-name')].map(n => n.textContent)`);
  assert.deepEqual(names, [SAVED, ASKS]);
  assert.ok((await page.eval(`${compat}.querySelector('h2').textContent`)).includes("v1.2.0"));
  await page.waitFor(`${versionOf} === 'v1.2.0'`);
  assert.equal(await page.eval(badge("v1.2.0")), null);

  // Back to v1.0.0: the dialog closes and the old version is in use again; v1.2.0 stays on disk.
  await page.eval(`${button(compat, "Go back to v1.0.0")}.click()`);
  await page.waitFor(`${compat} === null && ${versionOf} === 'v1.0.0'`);
  assert.ok(existsSync(join(dir, "drivers", "adbc", "trino", "v1.2.0", "driver.toml")));

  // "Check for updates" on demand finds v1.2.0 again.
  assert.equal(await page.eval(badge("v1.2.0")), null);
  await page.eval(`document.querySelector('.drivers-panel [data-action="check-updates"]').click()`);
  await page.waitFor(`${badge("v1.2.0")} != null`);

  // Update again; this time the check is closed, not rolled back. With v1.2.0 in use, another
  // check says everything is up to date.
  await page.eval(`${button(row, "Update")}.click()`);
  await finishedCheck(page);
  await closeCheck(page);
  await page.waitFor(`${versionOf} === 'v1.2.0' && ${badge("v1.2.0")} === null`);
  await page.eval(`document.querySelector('.drivers-panel [data-action="check-updates"]').click()`);
  await page.waitFor(`document.querySelector('.drivers-panel [data-field="checked"]')?.textContent === 'All drivers are up to date.'`);

  // Versions…: the registry's versions newest first, with the installed and the active marked.
  await page.eval(`${row}.querySelector('[data-action="versions"]').click()`);
  await page.waitFor(`${row}.querySelectorAll('.driver-version').length === 4`);
  assert.deepEqual(await page.eval(versionStates), ["v1.2.0:active", "v1.1.0:available", "v1.0.0:installed", "v0.9.0:available"]);

  // A package whose hash does not match is refused, and nothing changes.
  await page.eval(`${button(versionRow("v0.9.0"), "Install")}.click()`);
  await page.waitFor(`${panelError} != null`);
  assert.ok((await page.eval(`${panelError}.textContent`)).includes("corrupted"));
  assert.equal(await page.eval(`${versionRow("v0.9.0")}.dataset.state`), "available");
  assert.equal(await page.eval(versionOf), "v1.2.0");

  // An earlier version is installed and becomes the active one; the check offers v1.2.0 back.
  await page.eval(`${button(versionRow("v1.1.0"), "Install")}.click()`);
  await finishedCheck(page);
  assert.ok((await page.eval(`${compat}.querySelector('[data-action="rollback"]').textContent`)).includes("v1.2.0"));
  await closeCheck(page);
  await page.waitFor(`${versionRow("v1.1.0")}.dataset.state === 'active' && ${versionOf} === 'v1.1.0'`);
  assert.equal(await page.eval(`${versionRow("v1.2.0")}.dataset.state`), "installed");

  // An installed earlier version is activated without downloading.
  await page.eval(`${button(versionRow("v1.0.0"), "Use this version")}.click()`);
  await finishedCheck(page);
  await closeCheck(page);
  await page.waitFor(`${versionRow("v1.0.0")}.dataset.state === 'active' && ${versionOf} === 'v1.0.0'`);

  // Deleting an installed version keeps the others; it is still offered by the registry.
  await page.eval(`${button(versionRow("v1.2.0"), "Delete")}.click()`);
  await page.waitFor(`${versionRow("v1.2.0")}.dataset.state === 'available'`);
  assert.ok(!existsSync(join(dir, "drivers", "adbc", "trino", "v1.2.0", "driver.toml")));
  assert.ok(existsSync(join(dir, "drivers", "adbc", "trino", "v1.1.0", "driver.toml")));

  // Deleting the active one (no connection is open) hands over to the newest left.
  await page.eval(`${button(versionRow("v1.0.0"), "Delete")}.click()`);
  await page.waitFor(`${versionRow("v1.1.0")}.dataset.state === 'active' && ${versionOf} === 'v1.1.0'`);
  assert.equal(await page.eval(`${versionRow("v1.0.0")}.dataset.state`), "available");

  // The last version goes: the driver is no longer listed.
  await page.eval(`${button(versionRow("v1.1.0"), "Delete")}.click()`);
  await page.waitFor(`${row} === null`);
  assert.ok(!existsSync(join(dir, "drivers", "adbc", "trino")));
}
