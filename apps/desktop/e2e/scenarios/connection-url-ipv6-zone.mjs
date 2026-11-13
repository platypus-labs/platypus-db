// URL-08 (low). An IPv6 link-local host with its zone cannot go through the URL mode: the
// standard form `postgres://u@[fe80::1%25eth0]:5432/db` (RFC 6874) is "invalid URL: invalid
// IPv6 address", although Host mode takes fe80::1%eth0; and that host, typed in Host mode,
// prints in URL mode as `[fe80::1%eth0]` (the `%` not encoded), a URL the field then refuses
// when it is edited.
// Steps: New connection > PostgreSQL > Connect by URL; type
// postgres://u@[fe80::1%25eth0]:5432/db.
// Expected: Host fe80::1%eth0; the printed URL `[fe80::1%25eth0]` reads back.
// Actual: "invalid URL: invalid IPv6 address".
// Suspected: crates/platypus-core/src/config.rs:366 (`Url::parse` refuses zones; no
// fallback) and crates/platypus-core/src/config/to_url.rs (authority does not encode `%`).
import assert from "node:assert/strict";
import { fill, formField, seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, generalFields, newUrlForm, toHost, toUrl, url, urlError } from "../connection-url-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  // Host mode first: the zone is taken there; its printed URL must read back.
  await newUrlForm(page, "PostgreSQL");
  await toHost(page);
  await page.eval(fill(formField("Host"), "fe80::1%eth0"));
  await toUrl(page);
  const printed = await page.eval(`${url}.value`);
  await page.eval(`${url}.focus()`);
  await enterUrl(page, printed);
  const again = await page.eval(urlError);
  await cancel(page);

  await newUrlForm(page, "PostgreSQL");
  await enterUrl(page, "postgres://u@[fe80::1%25eth0]:5432/db");
  const error = await page.eval(urlError);
  let host = null;
  if (!error) {
    await toHost(page);
    host = (await generalFields(page)).Host;
  }
  await cancel(page);
  assert.equal(error, "", "RFC 6874 zone refused");
  assert.equal(host, "fe80::1%eth0");
  assert.equal(again, "", `the URL printed for fe80::1%eth0 (${printed}) does not read back`);
}
