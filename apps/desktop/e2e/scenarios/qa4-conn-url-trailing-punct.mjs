// QA4-CONN-10 (low). A URL copied from a sentence, a .env line or a code block usually carries
// the punctuation after it: `postgres://u:p@db.example.com:5433/shop;` or `…/shop,`. Pasted in
// the form, the form fills in with Database "shop;" and the name "shop;": no error, and the
// connection later fails with `database "shop;" does not exist`.
// Steps: New connection, PostgreSQL; paste `postgres://u:p@db.example.com:5433/shop;` in Name.
// Expected: Database "shop" (the trailing `;` or `,` dropped), or an error on the URL.
// Actual: Database "shop;", Name "shop;".
// Code: apps/desktop/src/connectionUrl.ts (the paste handler's trim) / platypus-core URL parser
// (path not checked for trailing punctuation).
import assert from "node:assert/strict";
import { formField, newConnection, paste, seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  const failures = [];
  for (const tail of [";", ","]) {
    await newConnection(page, "PostgreSQL");
    await page.eval(paste(formField("Name"), `postgres://u:p@db.example.com:5433/shop${tail}`));
    await page.sleep(900);
    const database = await page.eval(`${formField("Database")}.value`);
    if (database !== "shop") failures.push(`"${tail}": Database is "${database}"`);
    await page.eval(`[...document.querySelectorAll('.connection-dialog button')].find(b => b.textContent.trim() === 'Cancel').click()`);
    await page.waitFor(`document.querySelector('.connection-dialog') === null && document.querySelector('.modal.driver-picker') === null`);
  }
  assert.deepEqual(failures, [], failures.join(" | "));
}
