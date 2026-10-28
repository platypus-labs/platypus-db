// The sample database of the welcome screen: a first click builds the SQLite platypus survey
// in the profile, saves its connection and adds its saved scripts, linked to it, then opens
// the first script; the script runs. A second click opens the same connection again and
// keeps the scripts the user changed. A failed build shows its error and saves nothing.
import assert from "node:assert/strict";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ALT } from "../cdp.mjs";
import { status, visibleTab } from "../perf.mjs";

const NAME = "Platypus survey (sample)";
const card = `[...document.querySelectorAll('.welcome-card')].find(c => c.textContent.includes('Try the sample database'))`;
const connections = `[...document.querySelectorAll('.sidebar .tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)}).length`;
const tabTitles = `[...document.querySelectorAll('.tab')].map(t => t.textContent.replace(/×$/, ''))`;
const scriptLabels = `[...document.querySelectorAll('.scripts-tree .tree-label')].map(l => l.textContent)`;
const toast = `document.querySelector('.toast')?.textContent ?? ''`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;

// No connection: the welcome screen shows from the start.
export const ready = `${card} !== undefined`;

/** Closes every tab, so the welcome screen shows again. */
async function closeTabs(page) {
  while (await page.eval(`document.querySelectorAll('.tab').length`) > 0) {
    const before = await page.eval(`document.querySelectorAll('.tab').length`);
    await page.eval(`document.querySelector('.tab [aria-label="Close tab"]').click()`);
    // An edited script asks first: its changes are dropped.
    await page.waitFor(
      `document.querySelectorAll('.tab').length < ${before} || [...document.querySelectorAll('.modal button')].some(b => /Don.t save|Discard/.test(b.textContent))`,
    );
    await page.eval(`[...document.querySelectorAll('.modal button')].find(b => /Don.t save|Discard/.test(b.textContent))?.click()`);
    await page.waitFor(`document.querySelectorAll('.tab').length < ${before}`);
  }
  await page.waitFor(`${card} !== undefined`);
}

export default async function (page, { dir }) {
  const db = join(dir, "samples", "wildlife.db");
  const scripts = join(dir, "scripts", "all", "Sample");

  // A file where the samples folder goes: the build fails, with its error, and saves nothing.
  writeFileSync(join(dir, "samples"), "in the way");
  await page.eval(`${card}.click()`);
  await page.waitFor(`${toast} !== ''`, 15_000);
  assert.match(await page.eval(toast), /samples/, "the error names the folder");
  assert.equal(await page.eval(connections), 0, "no connection saved");
  assert.equal(await page.eval(`document.querySelectorAll('.tab').length`), 0);
  rmSync(join(dir, "samples"));
  await page.eval(`document.querySelector('.toast')?.click()`);

  // The first click: database, connection, scripts, and the first script open.
  await page.eval(`${card}.click()`);
  await page.waitFor(`${tabTitles}.some(t => t.startsWith('Sightings per river.sql'))`, 30_000);
  await page.waitFor(`${connections} === 1`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent.includes('Where the platypuses are seen')`);
  assert.ok(existsSync(db), "the database is in the profile");
  for (const name of ["Sightings per river", "Most seen platypuses", "Weight by sex", "Surveys by researcher"]) {
    assert.ok(existsSync(join(scripts, `${name}.sql`)), `${name}.sql`);
  }
  await page.waitFor(`${scriptLabels}.includes('Sample') && ${scriptLabels}.includes('Weight by sex')`, 10_000);
  assert.ok(
    await page.eval(`[...document.querySelectorAll('.scripts-tree .tree-detail')].some(d => d.title.includes(${JSON.stringify(NAME)}))`),
    "the scripts open on the sample's connection",
  );

  // The script runs on the sample: 18 rivers.
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("x", "KeyX", 88, ALT);
  await page.waitFor(`${cancel}?.disabled === true && ${status} !== '' && !${status}.includes('Executing')`, 30_000);
  assert.match(await page.eval(status), /\b18 rows\b/);

  // A script the user changed is kept; a second click opens the same connection.
  writeFileSync(join(scripts, "Weight by sex.sql"), "-- mine");
  await closeTabs(page);
  await page.eval(`${card}.click()`);
  await page.waitFor(`${tabTitles}.some(t => t.startsWith('Sightings per river.sql'))`, 30_000);
  assert.equal(await page.eval(connections), 1, "one sample connection");
  assert.equal(readFileSync(join(scripts, "Weight by sex.sql"), "utf8"), "-- mine");

  // A script deleted by the user comes back with the next click; the others stay as they are.
  rmSync(join(scripts, "Surveys by researcher.sql"));
  await closeTabs(page);
  await page.eval(`${card}.click()`);
  await page.waitFor(`${tabTitles}.some(t => t.startsWith('Sightings per river.sql'))`, 30_000);
  assert.ok(existsSync(join(scripts, "Surveys by researcher.sql")));
  assert.equal(readFileSync(join(scripts, "Weight by sex.sql"), "utf8"), "-- mine");
}
