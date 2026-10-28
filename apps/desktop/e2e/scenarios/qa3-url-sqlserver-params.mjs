// URL-01 (medium). A `sqlserver://` URL in SQL Server's URL mode loses the parameters the
// SQL Server URLs of other tools carry (go-mssqldb, sqlx, Prisma): `?database=master` and
// `encrypt=true&trustServerCertificate=true` are "Not used here", although the same keys in a
// JDBC URL or an ADO.NET string set the database and the TLS mode. The connection then opens
// the login's default database with the default TLS mode instead of the ones asked for.
// Steps: New connection > SQL Server > Connect by URL; type
// sqlserver://sa@localhost:51433?database=master&encrypt=true&trustServerCertificate=true.
// Expected: Database "master", TLS "require" (encrypt, trusting the certificate), no note.
// Actual: Database empty, TLS unchanged, note "Not used here: database, encrypt,
// trustServerCertificate".
// Suspected: crates/platypus-core/src/config.rs:410 (`read_url` query keys: only the libpq
// ones), unlike crates/platypus-core/src/config/jdbc.rs:291.
import assert from "node:assert/strict";
import { seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, generalFields, leaveUrl, newUrlForm, notes, toHost, url, urlError } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newUrlForm(page, "SQL Server");
  await enterUrl(page, "sqlserver://sa@localhost:51433?database=master&encrypt=true&trustServerCertificate=true");
  assert.equal(await page.eval(urlError), "");
  const left = await page.eval(notes);
  await leaveUrl(page);
  const printed = await page.eval(`${url}.value`);
  await toHost(page);
  const fields = await generalFields(page);
  await cancel(page);
  assert.equal(fields["Database (optional)"], "master", `database= dropped (note: ${left})`);
  assert.match(printed, /sslmode=require/, `encrypt/trustServerCertificate dropped: ${printed} (note: ${left})`);
  assert.equal(left, "");
}
