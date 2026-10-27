// Regression guard: the SSL modes against the dev PostgreSQL, whose certificate is the
// self-signed "snakeoil" one: disable is plaintext, prefer and require are TLS, verify-ca and
// verify-full refuse the unknown issuer, a CA file that does not exist is named in the error,
// and half a client certificate is refused by Test and by Save.
// Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { button, dialog, fill, fillForm, formField, needsPort, newConnection, seedSqlite, testConnection } from "../conn-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await fillForm(page, { Name: "QA tls", Host: "localhost", Port: 55432, Database: "platypus", User: "platypus", Password: "platypus" });
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network')).click()`);
  const mode = (m) => page.eval(fill(formField("SSL mode"), m));

  await mode("disable");
  assert.match(await testConnection(page), /^Connected: PostgreSQL .* · not encrypted$/);
  for (const m of ["prefer", "require"]) {
    await mode(m);
    assert.match(await testConnection(page), /^Connected: PostgreSQL .* · TLS$/, m);
  }
  for (const m of ["verify-ca", "verify-full"]) {
    await mode(m);
    assert.match(await testConnection(page), /UnknownIssuer/, m);
  }
  await page.waitFor(`document.querySelector('[data-field="ssl_root_cert"]') !== null`);
  await page.eval(fill(`document.querySelector('[data-field="ssl_root_cert"]')`, "C:/qa_conn_missing/ca.pem"));
  assert.match(await testConnection(page), /cannot read C:\/qa_conn_missing\/ca\.pem/);

  await page.eval(fill(`document.querySelector('[data-field="ssl_root_cert"]')`, ""));
  await mode("require");
  await page.eval(fill(`document.querySelector('[data-field="ssl_client_cert"]')`, "C:/qa_conn_missing/client.crt"));
  const half = /a client certificate needs both the certificate and its key/;
  assert.match(await testConnection(page), half);
  // Save is refused with the same message (the notice may stay the same element): the form
  // stays open once Save has answered (its button enabled again).
  await page.eval(`${button(dialog, "Save")}.click()`);
  await page.sleep(300);
  await page.waitFor(`${dialog} === null || !${button(dialog, "Save")}.disabled`);
  assert.equal(await page.eval(`${dialog} !== null`), true, "saved with half a client certificate");
  assert.match(await page.eval(`document.querySelector('.connection-dialog .notice.error').textContent`), half);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null`);
}
