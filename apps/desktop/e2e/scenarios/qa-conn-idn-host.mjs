// CONN-23 (low). A host with non-ASCII letters in a pasted URL, `postgres://u@dé.example/db`,
// fills the Host field with `d%C3%A9.example`: percent-encoded, which no resolver knows. It
// should be the name as written (or its punycode).
// Steps: paste the URL in a new connection's picker.
// Expected: Host `dé.example` or its punycode (`xn--….example`). Actual: `d%C3%A9.example`.
// Suspected: crates/platypus-core/src/config.rs `read_url` (`url.host_str()` of a non-special
// scheme is the opaque, percent-encoded host).
import assert from "node:assert/strict";
import { dialog, formField, openPicker, paste, seedSqlite, urlField } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await openPicker(page);
  await page.eval(paste(`document.querySelector(${JSON.stringify(urlField)})`, "postgres://u@dé.example/db"));
  await page.waitFor(`${dialog} !== null`);
  const host = await page.eval(`${formField("Host")}.value`);
  assert.ok(host === "dé.example" || /^xn--[a-z0-9-]+\.example$/.test(host), host);
}
