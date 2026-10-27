// QA ui: the app's own validation messages stay in English (and lower case) in Italian.
//
// Steps: language Italiano; New project (the + of the rail), name of three spaces, Salva.
// Expected: an Italian message, e.g. "Il nome del progetto è obbligatorio".
// Actual: "project name is required", the store's error text as it is. The same for "folder
// name is required", "connection name is required", "profile name is required" and the
// other StoreError::Invalid texts: they are not messages from a database, which the language
// setting says stay in theirs.
// Code: crates/platypus-store/src/connections.rs (`invalid("project name is required")`)
// shown raw by projects/ProjectDialog.tsx (`toCommandError(err).message`).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page) {
  await page.waitFor(`document.documentElement.lang === 'it'`);
  await page.eval(`document.querySelector('.project-add').click()`);
  await page.waitFor(`document.querySelector('.project-dialog input') === document.activeElement`);
  await page.type("   ");
  await page.eval(`document.querySelector('.project-dialog button[type=submit]').click()`);
  await page.waitFor(`document.querySelector('.project-dialog .notice') !== null`);
  const message = await page.eval(`document.querySelector('.project-dialog .notice').textContent`);
  assert.doesNotMatch(message, /project name is required/, `shown: ${message}`);
}
