// URL-05 (medium). A URL that does not read, typed in the URL mode, vanishes when the field is
// left: Tab (or a click elsewhere) puts back the previous URL and hides the error, with nothing
// to say that what was typed was thrown away. A user who mistyped a port, pressed Tab to go to
// the password, sees a valid-looking URL that is not the one typed.
// Steps: New connection > PostgreSQL > Connect by URL; type
// postgres://app@db.example:65536/shop (port out of range); press Tab.
// Expected: the field keeps the typed text and its error (aria-invalid), until corrected.
// Actual: the field shows postgres://localhost:5432/ again, no error.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx:511-515 (onBlur drops the
// draft and the error).
import assert from "node:assert/strict";
import { seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, newUrlForm, tab, url, urlError } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

const TYPED = "postgres://app@db.example:65536/shop";

export default async function (page) {
  await newUrlForm(page, "PostgreSQL");
  await page.eval(`${url}.focus()`);
  await enterUrl(page, TYPED);
  assert.match(await page.eval(urlError), /port/i);
  assert.equal(await page.eval(`${url}.getAttribute('aria-invalid')`), "true");
  await tab(page);
  await page.sleep(200);
  const value = await page.eval(`${url}.value`);
  const error = await page.eval(urlError);
  await cancel(page);
  assert.equal(value, TYPED, "the typed URL was replaced on leaving the field");
  assert.notEqual(error, "", "the error went away with the text");
}
