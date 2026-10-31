// The interface in Italian, Spanish, German, French, Portuguese and Chinese (the seven languages
// with English; the list offers exactly those): chosen in Settings ›
// Appearance › Language, each loads its dictionary and shows at once (the Settings' own
// title, the welcome screen), sets the page's language, and writes the keys of the
// shortcuts as its keyboards do (Strg in German). Back to English at the end; a start in a
// language not in the bundle shows it from the first frame.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { CTRL } from "../cdp.mjs";
import { restart } from "../qa-jrn.mjs";
import { sqliteConnection } from "../query.mjs";

const NAME = "E2E languages";
const LANGUAGES = ["it", "es", "de", "fr", "pt", "zh"];
const dictionary = async (lang) =>
  (await import(pathToFileURL(join(import.meta.dirname, "..", "..", "src", "i18n", `${lang}.ts`)).href)).default;
const pick = (value) => `(() => {
  const row = [...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('select option[value="es"]'));
  const el = row.querySelector('select');
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;
const heading = `(document.querySelector('.modal.settings h2')?.textContent ?? '')`;
const welcome = `(document.querySelector('.welcome')?.textContent ?? '')`;
const paletteKeys = `([...document.querySelectorAll('.welcome-keys kbd')][0]?.textContent ?? '')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "languages.db",
    id: "00000000-0000-4000-8000-00000000e301",
    name: NAME,
    sql: "CREATE TABLE t (id INTEGER PRIMARY KEY);",
  });
}

/** Opens the Settings on Appearance, picks `lang` and waits for the title in it. */
async function choose(page, lang, title) {
  if ((await page.eval(`document.querySelector('.modal.settings') === null`)) === true) {
    await page.key(",", "Comma", 188, CTRL);
    await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  }
  await page.settingsTab("Appearance", "Language and text");
  if (lang === "it") {
    // The list offers English, the six dictionaries and the system's language, no more.
    const offered = await page.eval(`[...[...document.querySelectorAll('.modal.settings .setting')].find(s => s.querySelector('select option[value="es"]')).querySelectorAll('select option')].map(o => o.value)`);
    assert.deepEqual([...offered].sort(), ["de", "en", "es", "fr", "it", "pt", "system", "zh"]);
  }
  await page.eval(pick(lang));
  await page.waitFor(`${heading} === ${JSON.stringify(title)}`, 10_000);
}

/** Closes the Settings with its button, named `label` in the language shown. */
async function done(page, label) {
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === ${JSON.stringify(label)}).click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

export default async function (page, ctx) {
  await page.waitFor(`${welcome} !== ''`);
  for (const lang of LANGUAGES) {
    const words = await dictionary(lang);
    await choose(page, lang, words["Settings"]);
    assert.equal(await page.eval(`document.documentElement.lang`), lang);
    // The Settings dialog: its sections are named in the language too.
    assert.ok(
      await page.eval(`document.querySelector('.modal.settings').textContent.includes(${JSON.stringify(words["Appearance"])})`),
      `${lang}: the Appearance section`,
    );
    await done(page, words["Done"]);
    await page.waitFor(`${welcome}.includes(${JSON.stringify(words["Try the sample database"])})`);
    assert.ok(
      (await page.eval(welcome)).includes(words["Open a data file"]),
      `${lang}: the welcome screen`,
    );
    const keys = await page.eval(paletteKeys);
    assert.equal(keys, lang === "de" ? "Strg+K" : "Ctrl+K", `${lang}: the palette's shortcut`);
  }

  // Started in Chinese: the first frame is already in it.
  const zh = await dictionary("zh");
  writeFileSync(join(ctx.dir, "settings.toml"), '[appearance]\nlanguage = "zh"\n');
  await restart(page, ctx, { ready: `document.querySelector('.welcome') !== null` });
  assert.ok((await page.eval(welcome)).includes(zh["Try the sample database"]), "Chinese from the start");

  // Back to English.
  await choose(page, "en", "Settings");
  await done(page, "Done");
  await page.waitFor(`${welcome}.includes('Try the sample database')`);
  assert.equal(await page.eval(`document.documentElement.lang`), "en");
}
