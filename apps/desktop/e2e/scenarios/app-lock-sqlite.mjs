// The app's lock: without a master password, Lock PlatypusDB says what it needs; with one,
// the window hides behind it, a wrong password is refused and the right one unlocks.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { palette } from "../plans.mjs";

const lock = `document.querySelector('.lock-screen')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  new DatabaseSync(join(dir, "a.db")).close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000941"
name = "Lock db"
driver = "sqlite"
database = '${join(dir, "a.db")}'
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

export default async function (page) {
  await page.waitFor(`${row("Lock db")} !== undefined`);
  await palette(page, "Lock PlatypusDB");
  await page.waitFor(`document.body.textContent.includes('Locking needs a master password')`, 15_000);
  assert.equal(await page.eval(`${lock}`), null);

  await page.eval(`window.__TAURI_INTERNALS__.invoke('enable_vault', { password: 'platypus-e2e' })`);
  await palette(page, "Lock PlatypusDB");
  await page.waitFor(`${lock} !== null`, 15_000);
  await unlockWith(page, "wrong");
  await page.waitFor(`${lock}.querySelector('.notice.error') !== null`, 15_000);
  await unlockWith(page, "platypus-e2e");
  await page.waitFor(`${lock} === null`, 15_000);
  await page.eval(`window.__TAURI_INTERNALS__.invoke('disable_vault', { password: 'platypus-e2e' })`);
}
