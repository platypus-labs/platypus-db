// URL-07 (low). Enter pressed in the URL field right after a paste saves the connection
// without the pasted URL: the form is submitted with the settings from before the backend
// has read the URL (a password manager or a quick paste-and-Enter does this).
// Steps: New connection > PostgreSQL; name "QA enter"; Connect by URL; paste
// postgres://app@db.example:6543/shop and press Enter at once.
// Expected: saved on db.example:6543, database shop.
// Actual: saved on localhost:5432 with no database.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx:223 (`save` does not wait for
// the URL being read in `editUrl`, :290).
import assert from "node:assert/strict";
import { dialog, fill, formField, seedSqlite } from "../conn-helpers.mjs";
import { newUrlForm, press, savedBlock, url } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page, { dir }) {
  await newUrlForm(page, "PostgreSQL");
  await page.eval(fill(formField("Name"), "QA enter"));
  await page.eval(`${url}.focus()`);
  await page.type("postgres://app@db.example:6543/shop");
  await press(page, "Enter", "Enter", 13);
  await page.waitFor(`${dialog} === null`, 15_000);
  const block = savedBlock(dir, "QA enter");
  assert.match(block, /host = "db.example"/, block);
  assert.match(block, /port = 6543/);
  assert.match(block, /database = "shop"/);
}
