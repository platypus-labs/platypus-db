// The app's lock after idle minutes (Settings › Lock the app after this many idle minutes, set
// to 1 here; the clock the page reads is moved forward so the test does not wait a minute):
// with no master password an idle app does not lock; once there is one it locks within the
// check interval, closing the production connection's navigator and leaving the development
// one (reconnecting only when it is used again, which the Activity log shows); a wrong password is refused and the right one unlocks; the user's activity (a mouse
// move) restarts the count, so a pause shorter than the limit after it does not lock, and a
// longer one does; with the setting at 0 the app never locks.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { menu } from "../transfer-helpers.mjs";

const lock = `document.querySelector('.lock-screen')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const PASSWORD = "platypus-e2e";

export function setup({ dir }) {
  for (const name of ["dev.db", "prod.db"]) {
    const db = new DatabaseSync(join(dir, name));
    db.exec("CREATE TABLE things (id INTEGER PRIMARY KEY)");
    db.close();
  }
  writeFileSync(join(dir, "settings.toml"), "[safety]\nlock_minutes = 1\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000951"
name = "Dev db"
driver = "sqlite"
database = '${join(dir, "dev.db")}'
connection_type = "dev"
save_password = false

[[connections]]
id = "00000000-0000-4000-8000-000000000952"
name = "Prod db"
driver = "sqlite"
database = '${join(dir, "prod.db")}'
connection_type = "prod"
save_password = false
`,
  );
}

async function unlockWith(page, password) {
  await page.eval(`(() => {
    const input = ${lock}.querySelector('input[type=password]');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, ${JSON.stringify(password)});
    input.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  await page.eval(`${lock}.querySelector('button[type=submit]').click()`);
}

/** Moves the page's clock forward by `ms` more. */
const skew = (page, ms) => page.eval(`window.__skew += ${ms}`);

/** Waits past one check of the idle timer (every 15 s). */
const pastOneCheck = (page) => page.sleep(16_500);

/** How many times the Activity log says each connection was connected. */
async function connects(page) {
  const entries = await page.eval(`(async () => {
    document.querySelector('.activity-button').click();
    await new Promise((r) => setTimeout(r, 300));
    const list = [...document.querySelectorAll('.activity-list > *')].map((e) => e.textContent);
    [...document.querySelectorAll('.modal.activity button')].find((b) => b.textContent === 'Done').click();
    await new Promise((r) => setTimeout(r, 200));
    return JSON.stringify(list);
  })()`);
  const count = (name) => JSON.parse(entries).filter((e) => e.includes(`Connect to ${name}`)).length;
  return { "Prod db": count("Prod db"), "Dev db": count("Dev db") };
}

/** Makes the page's clock movable with `skew`. */
const installSkew = (page) =>
  page.eval(`(() => {
    const real = Date.now.bind(Date);
    window.__skew = 0;
    Date.now = () => real() + window.__skew;
  })()`);

export default async function (page) {
  // The window may be behind others on the machine running the tests: a hidden page's timers are
  // slowed down, and the idle check is a timer.
  await page.send("Emulation.setFocusEmulationEnabled", { enabled: true }).catch(() => {});
  await page.waitFor(`${row("Dev db")} !== undefined && ${row("Prod db")} !== undefined`);
  for (const name of ["Dev db", "Prod db"]) {
    await page.eval(`${row(name)}.querySelector('.twisty').click()`);
    await page.waitFor(`${row(name)}.getAttribute('aria-expanded') === 'true' && ${row("things")} !== undefined`, 15_000);
  }
  await installSkew(page);

  // Two minutes idle and no master password: nothing to lock with, the app stays open.
  await skew(page, 120_000);
  await pastOneCheck(page);
  assert.equal(await page.eval(`${lock} === null`), true, "the app stays open");

  // With a master password the next check locks it; the production connection is closed, the
  // development one stays open.
  await page.eval(`window.__TAURI_INTERNALS__.invoke('enable_vault', { password: ${JSON.stringify(PASSWORD)} })`);
  await page.waitFor(`${lock} !== null`, 90_000);
  assert.deepEqual(await connects(page), { "Prod db": 1, "Dev db": 1 });
  await unlockWith(page, "not the password");
  await page.waitFor(`${lock}.querySelector('.notice.error') !== null`, 15_000);
  await unlockWith(page, PASSWORD);
  await page.waitFor(`${lock} === null`, 15_000);
  // Refreshing each connection: the production one connects anew, the development one still
  // has its session.
  for (const name of ["Prod db", "Dev db"]) {
    await menu(page, name, "Refresh");
    await page.sleep(1500);
  }
  assert.deepEqual(await connects(page), { "Prod db": 2, "Dev db": 1 });

  // The count starts again at the unlock: 25 s, a mouse move, 25 s more is 25 s idle, not 50
  // (and the 16 s of the wait are real time).
  await skew(page, 25_000);
  await page.eval(`window.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }))`);
  await skew(page, 25_000);
  await pastOneCheck(page);
  assert.equal(await page.eval(`${lock} === null`), true, "the mouse move restarted the count");
  // 40 s more after that is over a minute without the user: it locks.
  await skew(page, 40_000);
  await page.waitFor(`${lock} !== null`, 90_000);
  await unlockWith(page, PASSWORD);
  await page.waitFor(`${lock} === null`, 15_000);

  // 0 never locks, however long the pause.
  await page.eval(`window.__TAURI_INTERNALS__.invoke('set_setting', { key: 'safety.lock_minutes', value: 0 })`);
  // The page keeps the settings it loaded at the start: a reload makes it read this one.
  await page.eval(`location.reload()`);
  await page.waitFor(`${row("Dev db")} !== undefined && ${lock} === null`, 30_000);
  await installSkew(page);
  await skew(page, 3_600_000);
  await pastOneCheck(page);
  assert.equal(await page.eval(`${lock} === null`), true, "the app stays open");
  await page.eval(`window.__TAURI_INTERNALS__.invoke('disable_vault', { password: ${JSON.stringify(PASSWORD)} })`);
}
