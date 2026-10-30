// The startup check runs at most once a day: with the check made a moment ago (the profile has
// its `last-update-check`), the page shows no "update available" badge though the fake registry
// (PLATYPUS_DRIVER_REGISTRY) has a newer Trino, and the file keeps its time; "Check for updates"
// asks anyway, shows the badge and renews the time.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { badge, fakeRegistry, seedDriver, trinoConnections, versionOf } from "../drivers-registry.mjs";

let registry = "";
const NOTED = 4_102_444_800; // 2100-01-01: later than any clock the test sees.

export async function setup({ dir }) {
  registry = await fakeRegistry({ versions: ["v1.0.0", "v1.2.0"] });
  seedDriver(dir, "v1.0.0");
  trinoConnections(dir);
  mkdirSync(join(dir, "drivers"), { recursive: true });
  writeFileSync(join(dir, "drivers", "last-update-check"), String(Math.floor(Date.now() / 1000)));
}

export const env = () => ({ PLATYPUS_DRIVER_REGISTRY: registry });

export default async function (page, { dir }) {
  const file = join(dir, "drivers", "last-update-check");
  const before = readFileSync(file, "utf8");
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`document.querySelector('button[aria-label="Settings"]').click()`);
  await page.settingsTab("Drivers");
  await page.waitFor(`${versionOf} === 'v1.0.0'`);
  await page.sleep(2000);
  assert.equal(await page.eval(badge("v1.2.0")), null);
  assert.equal(readFileSync(file, "utf8"), before);

  // A clock that went back (the noted time is in the future) counts as due: the next start
  // asks again; here the click does, and renews the time.
  writeFileSync(file, String(NOTED));
  await page.eval(`document.querySelector('.drivers-panel [data-action="check-updates"]').click()`);
  await page.waitFor(`${badge("v1.2.0")} != null`);
  assert.notEqual(readFileSync(file, "utf8"), String(NOTED));
}
