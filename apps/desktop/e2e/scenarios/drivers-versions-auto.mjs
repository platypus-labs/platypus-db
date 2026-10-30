// Automatic driver updates: with "Install driver updates by itself" on, the startup check
// downloads and installs Trino v1.2.0 over the v1.0.0 the profile has (against a fake registry
// served locally, PLATYPUS_DRIVER_REGISTRY), keeps v1.0.0, and, as nobody asked for it, asks
// before connecting to the saved connections. "Check connections" runs the probe (the fake
// library cannot be loaded, so the saved connection fails and the one that asks for its password
// is left alone) and the way back returns to v1.0.0.
import assert from "node:assert/strict";
import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ASKS, SAVED, badge, button, compat, fakeRegistry, seedDriver, trinoConnections, versionOf } from "../drivers-registry.mjs";

let registry = "";

export async function setup({ dir }) {
  registry = await fakeRegistry({ versions: ["v1.0.0", "v1.2.0"] });
  seedDriver(dir, "v1.0.0");
  trinoConnections(dir);
  writeFileSync(join(dir, "settings.toml"), "[drivers]\nauto_update = true\n");
}

export const env = () => ({ PLATYPUS_DRIVER_REGISTRY: registry });

export default async function (page, { dir }) {
  // Nothing was clicked: the update was installed by itself, and the dialog asks first.
  await page.waitFor(`${compat}?.querySelector('[data-field="ask"]') != null`, 30_000);
  const ask = await page.eval(`${compat}.querySelector('[data-field="ask"]').textContent`);
  assert.ok(ask.includes("v1.2.0") && ask.includes("1 saved connection"), ask);
  assert.ok(existsSync(join(dir, "drivers", "adbc", "trino", "v1.2.0", "driver.toml")));
  assert.ok(existsSync(join(dir, "drivers", "adbc", "trino", "v1.0.0", "driver.toml")));
  // No connection was tried yet.
  assert.equal(await page.eval(`${compat}.querySelectorAll('.multi-run-row').length`), 0);

  await page.eval(`${button(compat, "Check connections")}.click()`);
  await page.waitFor(`${compat}.querySelector('[data-field="summary"]') != null`, 20_000);
  const names = await page.eval(`[...${compat}.querySelectorAll('.multi-run-name')].map(n => n.textContent)`);
  assert.deepEqual(names, [SAVED, ASKS]);
  assert.deepEqual(await page.eval(`[...${compat}.querySelectorAll('.multi-run-row')].map(r => r.dataset.state)`), ["failed", "skipped"]);

  // The way back: v1.0.0 is in use again, and no update is pending in the page (it was
  // installed, and the next check is a day away).
  await page.eval(`${button(compat, "Go back to v1.0.0")}.click()`);
  await page.waitFor(`${compat} === null`);
  await page.eval(`document.querySelector('button[aria-label="Settings"]').click()`);
  await page.settingsTab("Drivers");
  await page.waitFor(`${versionOf} === 'v1.0.0'`);
  assert.equal(await page.eval(badge("v1.2.0")), null);
}
