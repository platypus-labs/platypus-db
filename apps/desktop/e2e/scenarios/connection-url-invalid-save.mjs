// URL-06 (high). Save (and Test connection) with a URL that does not read in the URL mode go
// ahead with the settings from before it: the click leaves the field, which drops the error,
// and the connection is saved as the defaults (localhost:5432, no database) without a word.
// The same for a URL for another database ("This URL is for MySQL").
// Steps: New connection > PostgreSQL; name "QA bad url"; Connect by URL; type
// postgres://app@db.example:5432x/shop; click Save.
// Expected: the form stays open and says the URL is not valid; nothing saved.
// Actual: the form closes; connections.toml has "QA bad url" on host "localhost", port 5432.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx:223 (`save` ignores
// `urlError`) and :511 (onBlur clears it before the click lands).
import assert from "node:assert/strict";
import { dialog, fill, formField, seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, newUrlForm, savedBlock, url, urlError } from "../connection-url-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page, { dir }) {
  await newUrlForm(page, "PostgreSQL");
  await page.eval(fill(formField("Name"), "QA bad url"));
  await page.eval(`${url}.focus()`);
  await enterUrl(page, "postgres://app@db.example:5432x/shop");
  assert.notEqual(await page.eval(urlError), "");

  // A real click: the field loses the focus first.
  await page.clickSel('.connection-dialog button[type=submit]');
  await page.sleep(1500);
  const open = await page.eval(`${dialog} !== null`);
  if (open) await cancel(page);
  assert.equal(open, true, `saved anyway: ${savedBlock(dir, "QA bad url").replace(/\n/g, " ")}`);
  assert.equal(savedBlock(dir, "QA bad url"), "");
}
