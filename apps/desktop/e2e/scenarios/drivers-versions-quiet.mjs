// No update check by itself: with "Look for driver updates at startup" off, the page shows no
// "update available" badge though the fake registry (PLATYPUS_DRIVER_REGISTRY) has a newer
// Trino, and nothing is noted of a check; "Check for updates" asks the registry anyway and
// shows it.
import assert from "node:assert/strict";
import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { badge, fakeRegistry, row, seedDriver, trinoConnections, versionOf } from "../drivers-registry.mjs";

let registry = "";

export async function setup({ dir }) {
  registry = await fakeRegistry({ versions: ["v1.0.0", "v1.2.0"] });
  seedDriver(dir, "v1.0.0");
  trinoConnections(dir);
  writeFileSync(join(dir, "settings.toml"), "[drivers]\ncheck_updates = false\n");
}

export const env = () => ({ PLATYPUS_DRIVER_REGISTRY: registry });

export default async function (page, { dir }) {
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`document.querySelector('button[aria-label="Settings"]').click()`);
  await page.settingsTab("Drivers");
  await page.waitFor(`${versionOf} === 'v1.0.0'`);
  // Give a startup check, if there were one, time to answer.
  await page.sleep(2000);
  assert.equal(await page.eval(badge("v1.2.0")), null);
  assert.ok(!existsSync(join(dir, "drivers", "last-update-check")));

  // On demand it asks, whatever the setting says, and notes that it did.
  await page.eval(`document.querySelector('.drivers-panel [data-action="check-updates"]').click()`);
  await page.waitFor(`${badge("v1.2.0")} != null`);
  assert.ok(existsSync(join(dir, "drivers", "last-update-check")));
  assert.equal(await page.eval(`${row}.querySelector('[data-field="version"]').textContent`), "v1.0.0");
}
