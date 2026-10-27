// CONN-04 (medium, security). A pasted Turso URL (`libsql://db-acme.turso.io?authToken=…`)
// opens the form with SSL mode `prefer`: TLS without verifying the certificate, and plain
// HTTP when HTTPS cannot be reached, so the auth token can go to an impostor or in clear.
// The Turso tile itself presets `verify-full`; `libsql://` means TLS (it is `https`).
// Steps: New connection, paste the URL in the picker's URL field, open the Network tab.
// Expected: SSL mode verify-full (as the Turso / libSQL tile), unless the URL says otherwise.
// Actual: prefer.
// Suspected: crates/platypus-core/src/config.rs `read_url` (libSQL keeps SslMode::default()).
import assert from "node:assert/strict";
import { dialog, formField, openPicker, paste, seedSqlite, urlField } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await openPicker(page);
  await page.eval(paste(`document.querySelector(${JSON.stringify(urlField)})`, "libsql://db-acme.turso.io?authToken=tok"));
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await page.eval(`document.querySelector('.connection-dialog .dialog-header .driver-icon').getAttribute('aria-label')`), "Turso / libSQL");
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network')).click()`);
  await page.waitFor(`${formField("SSL mode")} !== undefined`);
  assert.equal(await page.eval(`${formField("SSL mode")}.value`), "verify-full");
}
