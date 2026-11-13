// URL-11 (low). With the interface in Italian the URL mode is translated ("Connetti con",
// the tooltip, "Questo URL è per MySQL…"), but the errors of a URL that does not read stay in
// English under the Italian field: "invalid URL: invalid port number", "unsupported URL scheme
// `htp`", "not a connection URL (postgres://…, jdbc:…) or connection string …".
// Steps: Italian; Nuova connessione > PostgreSQL > Connetti con: URL; type
// postgres://u@db.example:70000/shop.
// Expected: an Italian message (e.g. "URL non valido: numero di porta non valido").
// Actual: "invalid URL: invalid port number".
// Suspected: crates/platypus-core/src/config.rs:366 and the other `Error::Config` texts of
// the URL parsing, shown as they are by ConnectionDialog.tsx:312 (no translation step, as
// apps/desktop/src/storeErrors.ts does for the store's errors). See also JRN-07.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { italian } from "../journey-helpers.mjs";
import { button, dialog, fill, picker, seedSqlite } from "../conn-helpers.mjs";
import { byHost, byUrl, url, urlError } from "../connection-url-helpers.mjs";

const IT = italian();

export function setup({ dir }) {
  seedSqlite(dir);
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

async function typed(page, text) {
  await page.eval(fill(url, text));
  await page.sleep(500);
  return page.eval(urlError);
}

export default async function (page) {
  await page.eval(`document.querySelector('button[title="${IT["New connection"]}"]').click()`);
  await page.waitFor(`${picker} !== null`);
  await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === 'PostgreSQL').click()`);
  await page.waitFor(`${dialog} !== null && ${byUrl} !== null`);
  assert.equal(await page.eval(`document.querySelector('.connect-by > span').textContent`), IT["Connect by"]);
  assert.equal(await page.eval(`document.querySelector('.connection-dialog [role=radiogroup]').getAttribute('aria-label')`), IT["Connect by"]);
  assert.equal(await page.eval(`${byHost}.textContent`), "Host");
  await page.eval(`${byUrl}.click()`);
  await page.waitFor(`${url} !== null`);
  assert.equal(await page.eval(`${url}.title`), IT["The password goes to the Password field, not in the URL."]);
  await page.eval(`${url}.focus()`);

  // The form's own message is Italian.
  assert.equal(await typed(page, "mysql://root@db.example/shop"), "Questo URL è per MySQL: torna indietro per sceglierlo.");

  // The parser's are not.
  const errors = [];
  for (const text of ["postgres://u@db.example:70000/shop", "htp://db.example/shop", "testo qualsiasi"]) errors.push(await typed(page, text));
  await page.eval(`${button(dialog, IT["Cancel"])}.click()`);
  await page.waitFor(`${dialog} === null`);
  for (const e of errors) assert.doesNotMatch(e, /invalid URL|unsupported URL scheme|not a connection URL|invalid port/, `English error in the Italian interface: ${e}`);
}
