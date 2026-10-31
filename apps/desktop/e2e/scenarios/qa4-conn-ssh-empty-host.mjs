// QA4-CONN-1 (low). Test connection with the SSH tunnel ticked and no SSH host (Save refuses it
// with the field's own validation) skips that check and tries to reach "ops@:22": the button
// stays busy for the 10 s connect timeout and the answer is "SSH ops@:22: no answer within
// 10 s", as if a server had been slow. With an empty SSH user it tries "SSH @127.0.0.1:22".
// Steps: New connection, PostgreSQL, host db.internal; Network: tick the SSH tunnel, user
// ops, leave the SSH host empty; Test connection.
// Expected: at once, a message that the SSH host is missing (as for the port, CONN-13).
// Actual: ~10 s, then "SSH ops@:22: no answer within 10 s".
// Code: apps/desktop/src/components/ConnectionDialog.tsx `test()` (no validation of the
// tunnel's fields before calling the backend); crates/platypus-net (no check of an empty host).
import assert from "node:assert/strict";
import { fill, fillForm, formField, newConnection, seedSqlite, testConnection } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await fillForm(page, { Host: "db.internal", Database: "app", User: "me", Password: "x" });
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network')).click()`);
  await page.eval(`document.querySelector('[data-page="network"] .check input').click()`);
  await page.waitFor(`document.querySelector('.ssh-section') !== null`);
  await page.eval(fill(formField("SSH user", ".ssh-section"), "ops"));
  const started = Date.now();
  const message = await testConnection(page, 60_000);
  const seconds = (Date.now() - started) / 1000;
  assert.doesNotMatch(message, /ops@:22|no answer/, `an empty SSH host was tried (${seconds.toFixed(1)} s): ${message}`);
  assert.match(message, /host/i, message);
  assert.ok(seconds < 3, `answered after ${seconds.toFixed(1)} s: ${message}`);
}
