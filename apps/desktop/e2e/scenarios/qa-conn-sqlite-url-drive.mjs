// CONN-09 (medium). A SQLite URL with two slashes and a Windows drive, `sqlite://C:/data/x.db`
// (what many tools and users write), opens the form with the file `/data/x.db`: the drive
// letter is taken as the URL's host and dropped, so the connection silently points at
// another file (on the current drive).
// Steps: New connection, paste `sqlite://C:/data/x.db` in the picker's URL field.
// Expected: database file `C:/data/x.db`, or an error saying the URL needs three slashes.
// Actual: `/data/x.db`.
// Suspected: crates/platypus-core/src/config.rs `read_url` (embedded drivers ignore the host).
import assert from "node:assert/strict";
import { dialog, formField, openPicker, paste, seedSqlite, urlField } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  // SQLite has no URL mode: the file field is its address, and a URL pasted in the name
  // field is read into it, or refused.
  await newFromUrl(page, "sqlite://C:/data/x.db");
  const error = `document.querySelector('.connection-dialog .error-text, .connection-dialog .notice.error')`;
  if (await page.eval(`${error} !== null`)) return; // Refused: fine.
  assert.equal(await page.eval(`${formField("Database file")}.value`), "C:/data/x.db");
}
