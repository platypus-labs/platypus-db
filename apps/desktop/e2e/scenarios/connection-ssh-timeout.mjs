// CONN-16 (low). An SSH server that does not answer keeps Test connection busy for the OS's
// TCP timeout (about 21 s on Windows), twice the 10 s connect timeout the database itself
// gets; meanwhile Cancel is disabled, so the dialog cannot be closed by its buttons.
// Steps: New connection, PostgreSQL; Network: SSH tunnel through 10.255.255.1 (an address
// that drops packets), user ops; Test connection.
// Expected: an error within the connect timeout (~10 s). Actual: ~21 s.
// Suspected: crates/platypus-net (the SSH TCP connect has no timeout of its own).
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
  await page.eval(fill(`document.querySelector('.ssh-section input')`, "10.255.255.1"));
  await page.eval(fill(formField("SSH user", ".ssh-section"), "ops"));
  const started = Date.now();
  const message = await testConnection(page, 60_000);
  const seconds = (Date.now() - started) / 1000;
  assert.match(message, /SSH/);
  assert.ok(seconds < 13, `the SSH connect took ${seconds.toFixed(1)} s: ${message}`);
}
