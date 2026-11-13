// Regression guard: a connections.toml the app cannot read is reported in the navigator, and
// saving a new connection is refused with the same error instead of writing over the file:
// the user's connections are never lost to a typo in the file.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, fill, formField, newConnection } from "../conn-helpers.mjs";

const BROKEN = `version = 7\n\n[[connections]]\nid = "not-a-uuid"\nname = "precious"\ndriver = "postgres"\n`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), BROKEN);
}

// No connection row: the navigator shows the error instead.
export const ready = `document.body.innerText.includes('connections.toml')`;

export default async function (page, { dir }) {
  assert.match(await page.eval(`document.body.innerText`), /invalid .*connections\.toml: TOML parse error/);
  await newConnection(page, "SQLite");
  await page.eval(fill(formField("Name"), "QA new"));
  await page.eval(fill(formField("Database file"), join(dir, "x.db")));
  await page.eval(`${button(dialog, "Save")}.click()`);
  await page.waitFor(`document.querySelector('.connection-dialog .notice.error') !== null`);
  assert.match(await page.eval(`document.querySelector('.connection-dialog .notice.error').textContent`), /connections\.toml/);
  assert.equal(readFileSync(join(dir, "connections.toml"), "utf8"), BROKEN);
}
