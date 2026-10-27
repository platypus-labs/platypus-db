// CONN-22 (low). Parts of a pasted URL are dropped without the note the form shows for other
// left-out parameters ("Not used here: …"):
// - `jdbc:postgresql://h1:5432,h2:5432/app` (failover hosts) keeps h1 and silently loses h2
//   (the same libpq URL `postgres://u@h1:5432,h2:5433/db` is refused: "invalid port number");
// - `postgres://u@h/db?application_name=foo&connect_timeout=5`: both are read, then not
//   carried to the form.
// Steps: paste each in a new connection's picker.
// Expected: a "Not used here" note naming what was left out. Actual: no note.
// Suspected: crates/platypus-core/src/config/jdbc.rs (multi-host) and
// apps/desktop/src-tauri/src/commands/connections.rs `parse_connection_url` (ParsedForm has
// no application_name / connect_timeout).
import assert from "node:assert/strict";
import { button, dialog, openPicker, paste, picker, seedSqlite, urlField, newFromUrl } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

async function notesOf(page, url) {
  await newFromUrl(page, url);
  await page.sleep(200);
  const notes = await page.eval(`document.querySelector('.connection-dialog .url-notes')?.textContent ?? ''`);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null && ${picker} === null`);
  return notes;
}

export default async function (page) {
  assert.match(await notesOf(page, "jdbc:postgresql://h1:5432,h2:5432/app"), /h2/);
  assert.match(await notesOf(page, "postgres://u@h/db?application_name=foo&connect_timeout=5"), /application_name/);
}
