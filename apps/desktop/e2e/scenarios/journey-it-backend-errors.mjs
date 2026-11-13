// JRN-07 (low). With the interface in Italian, the app's own error messages made in the
// backend stay in English, next to Italian text: Prova la connessione on a SQLite file in a
// missing folder says "Z:\nope\x.db does not exist, nor does its folder" (the Settings say
// only the databases' messages stay in their language; this one is PlatypusDB's).
// Steps: Italian; Nuova connessione > SQLite; File del database "Z:\nope\x.db"; Prova la
// connessione.
// Expected: an Italian message ("… non esiste, e nemmeno la sua cartella"). Actual: English.
// Suspected: crates/platypus-driver-sqlite/src/lib.rs:96 (and the other `Error::Connect`
// texts of the app), with no translation step like apps/desktop/src/storeErrors.ts.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { italian } from "../journey-helpers.mjs";
import { fill, seedSqlite } from "../conn-helpers.mjs";

const IT = italian();
const dialog = `document.querySelector('.connection-dialog')`;

export function setup({ dir }) {
  seedSqlite(dir);
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export default async function (page) {
  await page.eval(`document.querySelector('button[title="${IT["New connection"]}"]').click()`);
  await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`);
  await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === 'SQLite').click()`);
  await page.waitFor(`${dialog} !== null`);
  const file = `[...${dialog}.querySelectorAll('label.field')].find(l => l.querySelector('span')?.textContent === ${JSON.stringify(IT["Database file"])}).querySelector('input')`;
  await page.eval(fill(file, "Z:\\nope\\x.db"));
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(IT["Test connection"])}).click()`);
  await page.waitFor(`${dialog}.querySelector('.notice.error') !== null`, 15_000);
  const text = await page.eval(`${dialog}.querySelector('.notice.error').textContent`);
  assert.doesNotMatch(text, /does not exist|nor does its folder/, `English message in the Italian interface: ${text}`);
}
