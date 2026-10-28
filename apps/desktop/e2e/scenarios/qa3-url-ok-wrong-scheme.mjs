// Regression guard: what the URL mode refuses, it says so under the field (role=alert,
// aria-invalid, aria-describedby) and changes nothing: a URL for another database names it
// (MySQL URL in PostgreSQL's form, PostgreSQL URL in MongoDB's, a MongoDB one in MySQL's), an
// unknown scheme, `http://` where it means nothing (PostgreSQL), text that is no URL, a port
// out of range, an unknown sslmode. An empty field is no error, and a valid URL clears the
// error.
import assert from "node:assert/strict";
import { seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, generalFields, newUrlForm, toHost, url, urlError } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

async function refused(page, text, pattern) {
  await enterUrl(page, text);
  const error = await page.eval(urlError);
  assert.match(error, pattern, `${text}: ${error}`);
  assert.equal(await page.eval(`${url}.getAttribute('aria-invalid')`), "true");
  assert.equal(await page.eval(`${url}.getAttribute('aria-describedby')`), "conn-url-error");
  assert.equal(await page.eval(`document.getElementById('conn-url-error').getAttribute('role')`), "alert");
  // The field keeps what was typed while it has the focus.
  assert.equal(await page.eval(`${url}.value`), text);
}

export default async function (page) {
  await newUrlForm(page, "PostgreSQL");
  await page.eval(`${url}.focus()`);
  await refused(page, "mysql://root@db.example:3306/shop", /This URL is for MySQL/);
  await refused(page, "mongodb://db.example/shop", /This URL is for MongoDB/);
  await refused(page, "htp://db.example/shop", /unsupported URL scheme `htp`/);
  await refused(page, "http://db.example:8123/shop", /unsupported URL scheme `http`/);
  await refused(page, "just some text", /not a connection URL/);
  await refused(page, "postgres://u@db.example:70000/shop", /port/);
  await refused(page, "postgres://u@db.example/shop?sslmode=sometimes", /sslmode/);

  // Empty: no error.
  await enterUrl(page, "");
  assert.equal(await page.eval(urlError), "");
  // A valid one clears it.
  await enterUrl(page, "postgres://u@db.example:5432/shop");
  assert.equal(await page.eval(urlError), "");
  assert.equal(await page.eval(`${url}.getAttribute('aria-invalid')`), "false");

  // Nothing of the refused URLs reached the settings.
  await toHost(page);
  const fields = await generalFields(page);
  assert.equal(fields.Host, "db.example");
  assert.equal(fields.Database, "shop");
  await cancel(page);

  await newUrlForm(page, "MongoDB");
  await page.eval(`${url}.focus()`);
  await refused(page, "postgres://u@db.example/shop", /This URL is for PostgreSQL/);
  await refused(page, "mongodb://", /no host/);
  await cancel(page);

  await newUrlForm(page, "MySQL");
  await page.eval(`${url}.focus()`);
  await refused(page, "jdbc:sqlserver://db.example:1433;databaseName=shop", /This URL is for SQL Server/);
  await cancel(page);
}
