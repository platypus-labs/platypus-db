// The master password against the dev PostgreSQL: turned on in the Settings, a saved
// password goes into the encrypted file; once locked, connecting asks for the master
// password; turned off, the file goes away. Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { skip as noPostgres } from "./dev-postgres.mjs";

const NAME = "E2E vault PG";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, text) =>
  `[...document.querySelectorAll(${JSON.stringify(`${scope} button`)})].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
/** Sets the field labelled `label` inside `scope` and tells React, as typing would. */
const setLabeled = (scope, label, value) => `(() => {
  const el = [...document.querySelectorAll(${JSON.stringify(`${scope} label.field`)})]
    .find(l => l.querySelector('span')?.textContent === ${JSON.stringify(label)}).querySelector('input');
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('input', { bubbles: true }));
})()`;

export const skip = noPostgres;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 3

[[connections]]
id = "00000000-0000-4000-8000-000000000009"
name = "${NAME}"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = true
`,
  );
}

/** Opens the Settings at the master password. */
async function openSettings(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Master password");
  await page.waitFor(`document.querySelector('.master-password button') !== null`);
}

async function closeSettings(page) {
  await page.eval(`${button(".modal.settings .actions", "Done")}.click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

export default async function (page, { dir }) {
  const vault = join(dir, "secrets.vault");
  await page.waitFor(`${row(NAME)} !== undefined`);

  // Turned on: an empty vault file.
  await openSettings(page);
  await page.eval(`${button(".master-password", "Set a master password…")}.click()`);
  await page.eval(setLabeled(".master-password", "New master password", "m1"));
  await page.eval(setLabeled(".master-password", "Repeat it", "m1"));
  await page.eval(`${button(".master-password", "Turn on")}.click()`);
  await page.waitFor(`document.querySelector('.master-password .notice.ok') !== null`, 15_000);
  assert.ok(existsSync(vault));
  await closeSettings(page);

  // A password saved from the connection dialog goes into it, encrypted.
  await page.eval(`[...${row(NAME)}.querySelectorAll('button')].find(b => b.title === 'Edit').click()`);
  await page.waitFor(`document.querySelector('.modal form, form.modal') !== null`);
  await page.eval(setLabeled(".modal", "Password", "platypus"));
  await page.eval(`${button(".modal .actions", "Save")}.click()`);
  await page.waitFor(`document.querySelector('.modal') === null`);
  const text = readFileSync(vault, "utf8");
  assert.ok(!text.includes("platypus"), text);

  // Locked: connecting asks for the master password, and refuses a wrong one.
  await openSettings(page);
  await page.eval(`${button(".master-password", "Lock now")}.click()`);
  await page.waitFor(`${button(".master-password", "Lock now")}?.disabled === true`);
  await closeSettings(page);
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.unlock-vault') !== null`, 15_000);
  await page.eval(setLabeled(".unlock-vault", "Master password", "nope"));
  await page.eval(`${button(".unlock-vault", "Unlock")}.click()`);
  await page.waitFor(`document.querySelector('.unlock-vault .notice.error')?.textContent.includes('wrong master password')`, 15_000);
  await page.eval(setLabeled(".unlock-vault", "Master password", "m1"));
  await page.eval(`${button(".unlock-vault", "Unlock")}.click()`);
  await page.waitFor(`document.querySelector('.unlock-vault') === null`, 15_000);
  await page.waitFor(`${row("demo")} !== undefined`, 15_000);

  // Turned off: the passwords go back to the keychain and the file is deleted.
  await openSettings(page);
  await page.eval(`${button(".master-password", "Turn off…")}.click()`);
  await page.eval(setLabeled(".master-password", "Current master password", "m1"));
  await page.eval(`${button(".master-password", "Turn off")}.click()`);
  await page.waitFor(`document.querySelector('.master-password .notice.ok')?.textContent.includes('keychain')`, 15_000);
  assert.ok(!existsSync(vault));
  await closeSettings(page);
}
