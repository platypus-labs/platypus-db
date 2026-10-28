// URL-02 (medium). libSQL's own clients write a local sqld without TLS as
// `libsql://localhost:8080?tls=0` (libsql-client-ts, libsql-experimental-python); typed in the
// libSQL form's URL mode, `tls=0` is "Not used here" and the connection keeps verified TLS,
// so Test connection to the plain-HTTP server fails.
// Steps: New connection > Turso / libSQL > Connect by URL; type
// libsql://localhost:58080?tls=0 (docker/compose.yml's sqld); Test connection.
// Expected: TLS off (the printed URL says sslmode=disable or http://), "Connected".
// Actual: note "Not used here: tls", TLS verify-full, the test fails.
// Suspected: crates/platypus-core/src/config.rs:410 (no `tls` key for libSQL).
import assert from "node:assert/strict";
import { needsPort, seedSqlite, testConnection } from "../conn-helpers.mjs";
import { LIBSQL_TOKEN } from "../qa-dbs.mjs";
import { cancel, enterUrl, newUrlForm, notes, urlError } from "../qa3-url.mjs";

export const skip = needsPort(58080, "libSQL");

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newUrlForm(page, "Turso / libSQL");
  await enterUrl(page, `libsql://localhost:58080?tls=0&authToken=${LIBSQL_TOKEN}`);
  assert.equal(await page.eval(urlError), "");
  const left = await page.eval(notes);
  const result = await testConnection(page, 30_000);
  await cancel(page);
  assert.equal(left, "", "tls=0 left out");
  assert.match(result, /^Connected/);
}
