// CONN-11 (medium). The same MongoDB URL authenticates against different databases depending
// on whether it carries the password: `mongodb://u:p@h/shop` gets `authSource=shop` (as the
// connection string spec and mongosh say: the path's database), but `mongodb://u@h/shop`,
// the usual form when the password is typed separately, gets no authSource and the driver
// authenticates against `admin`. Users created in `shop` cannot log in, and admin users get
// in where mongosh would refuse.
// Steps: paste `mongodb://u@h:27017/shop` in a new connection's picker.
// Expected: database `shop?authSource=shop` (as with the password in the URL).
// Actual: `shop`.
// Suspected: crates/platypus-core/src/config.rs `from_mongo_url` (adds authSource only when
// `config.password.is_some()`).
import assert from "node:assert/strict";
import { button, dialog, formField, openPicker, paste, picker, seedSqlite, urlField, newFromUrl } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

async function databaseOf(page, url) {
  await newFromUrl(page, url);
  const database = await page.eval(`${formField("Database (optional)")}.value`);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null && ${picker} === null`);
  return database;
}

export default async function (page) {
  assert.equal(await databaseOf(page, "mongodb://u:p@h:27017/shop"), "shop?authSource=shop");
  assert.equal(await databaseOf(page, "mongodb://u@h:27017/shop"), "shop?authSource=shop");
}
