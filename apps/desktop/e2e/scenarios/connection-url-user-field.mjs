// Regression guard: in URL mode the Authentication section still shows the User field. A URL
// fills it, a user typed there shows in the URL, and an emptied one leaves the URL without
// a user.
import assert from "node:assert/strict";
import { fill, formField, newConnection, seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, toUrl, url, urlError } from "../connection-url-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

const user = formField("User");

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await toUrl(page);
  assert.equal(await page.eval(`${user} !== undefined`), true, "no User field in URL mode");

  await enterUrl(page, "postgresql://ana@db.example/inventory");
  assert.equal(await page.eval(urlError), "");
  assert.equal(await page.eval(`${user}.value`), "ana");

  // Typed in the field: the URL follows.
  await page.eval(fill(user, "bob"));
  await page.waitFor(`${url}.value.startsWith('postgres://bob@db.example')`);
  assert.match(await page.eval(`${url}.value`), /\/inventory/);

  // Emptied: the URL has no user left.
  await page.eval(fill(user, ""));
  await page.waitFor(`!${url}.value.includes('@') && ${url}.value.startsWith('postgres://db.example')`);
  await cancel(page);
}
