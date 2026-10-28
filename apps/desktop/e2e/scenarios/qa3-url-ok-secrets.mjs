// Regression guard: passwords in a URL typed in the form's URL mode. Percent-encoded reserved
// characters (@ : / # ? % space) and UTF-8 are decoded into the Password field; a raw `@` or
// `:` in the password still finds the host after the last `@`; the URL field never shows the
// password, nor libSQL's auth token (`?authToken=`), which goes to the Auth token field; a
// MongoDB `password=` option is left out with a note. Saved, no file of the profile holds the
// password in clear.
import assert from "node:assert/strict";
import { fill, formField, saveForm, seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, filesHolding, generalFields, newUrlForm, notes, secretField, toHost, url, urlError } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

const ODD = "p@ss:w/rd#?%ü x";
const ENCODED = "p%40ss%3Aw%2Frd%23%3F%25%C3%BC%20x";

export default async function (page, { dir }) {
  await newUrlForm(page, "PostgreSQL");
  await enterUrl(page, `postgres://app:${ENCODED}@db.example:6543/shop`);
  assert.equal(await page.eval(urlError), "");
  assert.equal(await page.eval(`${secretField}.value`), ODD);
  assert.equal(await page.eval(`${secretField}.type`), "password");
  let shown = await page.eval(`${url}.value`);
  assert.equal(shown, "postgres://app@db.example:6543/shop");

  // A raw `@` and `:` in the password: the host is after the last `@`.
  await enterUrl(page, "postgres://app:se@cr:et@db.example:6543/shop");
  assert.equal(await page.eval(`${secretField}.value`), "se@cr:et");
  assert.equal(await page.eval(`${url}.value`), "postgres://app@db.example:6543/shop");

  // A user name with reserved characters stays the user, encoded in the URL.
  await enterUrl(page, "postgres://u%40corp%3Ax:pw@db.example:6543/shop");
  shown = await page.eval(`${url}.value`);
  assert.doesNotMatch(shown, /pw/);
  await toHost(page);
  const fields = await generalFields(page);
  assert.equal(fields.User, "u@corp:x");
  assert.equal(fields.Host, "db.example");
  await page.eval(fill(formField("Name"), "QA odd secret"));
  await page.eval(fill(secretField, ODD));
  await saveForm(page);
  assert.deepEqual(filesHolding(dir, ODD), [], "the password is in clear in the profile");
  assert.deepEqual(filesHolding(dir, ENCODED), [], "the encoded password is in clear in the profile");

  // libSQL: the token of the URL goes to the Auth token field, out of the URL.
  await newUrlForm(page, "Turso / libSQL");
  await enterUrl(page, "libsql://db-acme.turso.io?authToken=tok-123.abc");
  assert.equal(await page.eval(urlError), "");
  assert.equal(await page.eval(`${secretField}.value`), "tok-123.abc");
  assert.doesNotMatch(await page.eval(`${url}.value`), /tok-123/);
  await cancel(page);

  // MongoDB: the password of the userinfo to the field; a `password=` option left out.
  await newUrlForm(page, "MongoDB");
  await enterUrl(page, "mongodb://root:m%2Fpw%40x@mongo.example:27017/shop?authSource=admin&password=leak9");
  assert.equal(await page.eval(urlError), "");
  assert.equal(await page.eval(`${secretField}.value`), "m/pw@x");
  const mongo = await page.eval(`${url}.value`);
  assert.doesNotMatch(mongo, /leak9|m%2Fpw|password=/);
  assert.match(await page.eval(notes), /password/);
  await cancel(page);
}
