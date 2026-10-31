// QA4-CONN-7 (low). With the interface in Italian, the errors the app itself raises when a
// connection is tested stay in English (the form's own checks are translated: the port, the
// host): Redis "invalid Redis database `abc`: a number, such as 0", SQLite "cannot open D:/Temp:
// unable to open database file", PostgreSQL with no password "invalid configuration: password
// missing". (A server's own text, like PostgreSQL's "password authentication failed", is the
// server's and out of scope.)
// Steps: Italian; Nuova connessione > Redis, database "abc", Prova la connessione; SQLite with a
// folder as the file; PostgreSQL with an empty password against the dev server.
// Expected: Italian messages. Actual: the English ones above.
// Code: crates/platypus-driver-redis / -sqlite / platypus-core `Error::Config` texts, shown as
// they are by ConnectionDialog.tsx (no translation step as apps/desktop/src/storeErrors.ts).
// Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { dialog, fill, formField, needsPort, seedSqlite } from "../conn-helpers.mjs";
import { italian } from "../qa-jrn.mjs";

const IT = italian();
export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  seedSqlite(dir);
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export default async function (page, ctx) {
  const open = async (tile) => {
    await page.eval(`document.querySelector('button[title="${IT["New connection"]}"]').click()`);
    await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`);
    await page.waitFor(`[...document.querySelectorAll('.driver-tile')].some(b => b.textContent === ${JSON.stringify(tile)})`, 15_000);
    await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === ${JSON.stringify(tile)}).click()`);
    await page.waitFor(`${dialog} !== null`);
  };
  const set = (label, value) => page.eval(fill(`(${formField(label)} ?? ${formField(`${label} (facoltativo)`)})`, value));
  const test = async () => {
    await page.eval(`[...document.querySelectorAll('.connection-dialog button')].find(b => b.textContent.trim() === ${JSON.stringify(IT["Test connection"])}).click()`);
    const notice = `document.querySelector('.connection-dialog .notice.ok, .connection-dialog .notice.error')`;
    await page.waitFor(`${notice} !== null`, 30_000);
    const text = await page.eval(`${notice}.textContent`);
    await page.eval(`[...document.querySelectorAll('.connection-dialog button')].find(b => b.textContent.trim() === ${JSON.stringify(IT["Cancel"])}).click()`);
    await page.waitFor(`${dialog} === null`);
    return text;
  };
  const english = [];
  await open("Redis");
  await set("Host", "localhost");
  await set("Porta", "46379");
  await set("Database", "abc");
  english.push(await test());
  await open("SQLite");
  await set("File del database", ctx.dir);
  english.push(await test());
  await open("PostgreSQL");
  await set("Host", "localhost");
  await set("Porta", "55432");
  await set("Database", "platypus");
  await set("Utente", "platypus");
  english.push(await test());
  const bad = english.filter((m) => /invalid|cannot open|missing|unable to/.test(m));
  assert.deepEqual(bad, [], `English messages in the Italian interface: ${bad.join(" | ")}`);
}
