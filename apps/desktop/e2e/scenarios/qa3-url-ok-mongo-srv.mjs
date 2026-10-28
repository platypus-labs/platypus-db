// Regression guard: MongoDB connection strings in the URL mode. `mongodb+srv://` keeps its
// scheme, drops the port (SRV has none) and keeps the options; Host mode shows them in the
// Database field (srv=true, authSource as the path's database); back in URL mode the same
// string, without the password. A `mongodb://` with authSource and a replica set reads the
// same way, and a `tls=true&tlsCAFile=` pair reads into the TLS settings.
import assert from "node:assert/strict";
import { seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, generalFields, leaveUrl, newUrlForm, secretField, toHost, toUrl, url, urlError } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newUrlForm(page, "MongoDB");
  await enterUrl(page, "mongodb+srv://ana:s3cr%3At@cluster0.abc.mongodb.net/shop?retryWrites=true&w=majority");
  assert.equal(await page.eval(urlError), "");
  assert.equal(await page.eval(`${secretField}.value`), "s3cr:t");
  const printed = await page.eval(`${url}.value`);
  assert.match(printed, /^mongodb\+srv:\/\/ana@cluster0\.abc\.mongodb\.net\/shop\?/);
  assert.doesNotMatch(printed, /:27017|s3cr/);
  assert.match(printed, /retryWrites=true/);
  assert.match(printed, /w=majority/);
  await toHost(page);
  const fields = await generalFields(page);
  assert.equal(fields.Host, "cluster0.abc.mongodb.net");
  assert.match(fields["Database (optional)"], /^shop\?.*srv=true/);
  assert.match(fields["Database (optional)"], /authSource=shop/);
  await toUrl(page);
  assert.equal(await page.eval(`${url}.value`), printed);

  await enterUrl(page, "mongodb://ana@db1.example:27017/admin?authSource=admin&replicaSet=rs0&tls=true&tlsCAFile=C%3A%2Fcerts%2Fca.pem");
  assert.equal(await page.eval(urlError), "");
  await leaveUrl(page);
  const tls = await page.eval(`${url}.value`);
  assert.match(tls, /^mongodb:\/\/ana@db1\.example:27017\/admin\?/);
  assert.match(tls, /replicaSet=rs0/);
  assert.match(tls, /tls=true/);
  assert.match(tls, /tlsCAFile=C:\/certs\/ca\.pem/);
  assert.doesNotMatch(tls, /srv=true|retryWrites/);
  await cancel(page);
}
