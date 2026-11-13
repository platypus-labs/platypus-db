// URL-04 (medium). In the URL mode a TLS file parameter cannot be removed: after
// `?sslmode=verify-ca&sslrootcert=C:/certs/ca.pem` the URL retyped without `sslrootcert`
// keeps the old CA file (the printed URL shows it again once the field is left), and so do
// `sslcert` / `sslkey` and MongoDB's `tlsCAFile`. Removing `sslmode` does reset the mode.
// Steps: New connection > PostgreSQL > Connect by URL; type
// postgres://u@db.example:5432/app?sslmode=verify-ca&sslrootcert=C:/certs/ca.pem;
// then replace it with postgres://u@db.example:5432/app?sslmode=verify-ca; Tab away.
// Expected: no CA file (the URL field shows ...?sslmode=verify-ca).
// Actual: ...?sslmode=verify-ca&sslrootcert=C:/certs/ca.pem.
// Suspected: apps/desktop/src/connectionUrl.ts:70-72 (`parsed.ssl_root_cert ??
// conn.ssl_root_cert`, same for the client certificate and key).
import assert from "node:assert/strict";
import { seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, leaveUrl, newUrlForm, url, urlError } from "../connection-url-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

async function settle(page) {
  await leaveUrl(page);
  return page.eval(`${url}.value`);
}

export default async function (page) {
  await newUrlForm(page, "PostgreSQL");
  await enterUrl(page, "postgres://u@db.example:5432/app?sslmode=verify-ca&sslrootcert=C:/certs/ca.pem&sslcert=C:/certs/c.pem&sslkey=C:/certs/c.key");
  assert.equal(await page.eval(urlError), "");
  assert.match(await settle(page), /sslrootcert=C:\/certs\/ca\.pem/);

  // Removing the mode works.
  await page.eval(`${url}.focus()`);
  await enterUrl(page, "postgres://u@db.example:5432/app");
  const noMode = await settle(page);
  assert.doesNotMatch(noMode, /sslmode=/);

  // Removing the files does not.
  await page.eval(`${url}.focus()`);
  await enterUrl(page, "postgres://u@db.example:5432/app?sslmode=verify-ca");
  const after = await settle(page);
  await cancel(page);
  assert.equal(after, "postgres://u@db.example:5432/app?sslmode=verify-ca");
}
