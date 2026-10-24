// Upgrading from a version without the Platypus themes: a saved "dark" keeps its colors (now
// "Classic Dark") and the appearance cached by the old version, which lacks the new
// variables, is dropped instead of painted.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const cssVar = (name) => `getComputedStyle(document.documentElement).getPropertyValue(${JSON.stringify(name)}).trim()`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
  writeFileSync(join(dir, "settings.toml"), '[appearance]\ntheme = "dark"\n');
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page) {
  await page.waitFor(`${cssVar("--bg")} === '#1b1d21'`);
  assert.notEqual(await page.eval(cssVar("--bg-rail")), "");

  // What 0.2.0 cached: the old variables only.
  await page.eval(`localStorage.setItem('platypus.appearance.v1', JSON.stringify({ dark: true, vars: { '--bg': '#1b1d21' } }))`);
  await page.eval(`location.reload()`);
  await page.waitFor(`document.querySelector('.shell') !== null`);
  await page.waitFor(`${cssVar("--bg")} === '#1b1d21'`);
  assert.equal(await page.eval(`localStorage.getItem('platypus.appearance.v1')`), null);
  const cached = JSON.parse(await page.eval(`localStorage.getItem('platypus.appearance.v2')`));
  assert.equal(cached.vars["--bg"], "#1b1d21");
  assert.notEqual(cached.vars["--bg-rail"] ?? "", "");
}
