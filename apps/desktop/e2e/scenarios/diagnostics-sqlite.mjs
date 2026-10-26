// The log and crash reports: the app logs its start and the web view's unhandled errors to
// <profile>/logs/platypus.log; a crash left by the last run shows a banner offering the
// diagnostic package, which Dismiss forgets; Settings › Diagnostics shows the logs' folder.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E SQLite";
const banner = `document.querySelector('.crash-banner')`;
const button = (scope, text) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

export function setup({ dir }) {
  writeFileSync(join(dir, "e2e.db"), "");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000001"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "e2e.db")}'
save_password = false
`,
  );
  // What a panic of the last run leaves.
  mkdirSync(join(dir, "logs"), { recursive: true });
  writeFileSync(join(dir, "logs", "crash.txt"), "PlatypusDB panicked on thread main: e2e");
}

export default async function (page, { dir }) {
  const log = join(dir, "logs", "platypus.log");
  assert.match(readFileSync(log, "utf8"), /INFO PlatypusDB \S+ started \(windows /);

  await page.waitFor(`${banner} !== null`, 15_000);
  assert.match(await page.eval(`${banner}.textContent`), /closed unexpectedly last time/);
  await page.eval(`${button(banner, "Dismiss")}.click()`);
  await page.waitFor(`${banner} === null`);
  const until = Date.now() + 10_000;
  while (existsSync(join(dir, "logs", "crash.txt"))) {
    assert.ok(Date.now() < until, "crash.txt was not removed");
    await new Promise((r) => setTimeout(r, 100));
  }

  // An error nothing catches reaches the log.
  await page.eval(`setTimeout(() => { throw new Error("e2e unhandled") }, 0)`);
  const logged = Date.now() + 10_000;
  while (!readFileSync(log, "utf8").includes("ERROR web view: Uncaught Error: e2e unhandled")) {
    assert.ok(Date.now() < logged, readFileSync(log, "utf8"));
    await new Promise((r) => setTimeout(r, 100));
  }

  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Diagnostics");
  const field = `document.querySelector('.diagnostics-panel [data-field="log_dir"]')`;
  await page.waitFor(`${field} !== null`);
  assert.equal(await page.eval(`${field}.textContent`), join(dir, "logs"));
  assert.notEqual(await page.eval(`document.querySelector('[data-action="save-diagnostics"]')`), null);
}
