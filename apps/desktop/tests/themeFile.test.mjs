// Unit tests of theme files (src/themes/file.ts): what reading one keeps, skips and refuses,
// and what writing one gives back.
import assert from "node:assert/strict";
import { test } from "node:test";
import { readThemeFile, themeFileName, themeFileText } from "../src/themes/file.ts";

const KEYS = ["bg", "fg", "accent", "syn-keyword"];
const isColor = (v) => /^#[0-9a-f]{6}$/i.test(v) || /^rgba?\(/.test(v) || /^color-mix\(/.test(v);
const read = (doc) => readThemeFile(typeof doc === "string" ? doc : JSON.stringify(doc), KEYS, isColor);

test("a file keeps its known colors and lists what it skips", () => {
  const { theme, skipped } = read({
    platypus_theme: 1,
    name: "  Sea\u0007 ",
    dark: { bg: " #0b1d26 ", accent: "rgb(1, 2, 3)", glow: "#ffffff", fg: "blue-ish", "syn-keyword": 7 },
    extra: true,
  });
  assert.deepEqual(theme, { name: "Sea", dark: { bg: "#0b1d26", accent: "rgb(1, 2, 3)" } });
  assert.deepEqual(skipped.sort(), ["dark.fg: not a color", "dark.glow: unknown", "dark.syn-keyword: not a color", "extra: unknown"]);
});

test("both variants, or either, and a default name", () => {
  const both = read({ platypus_theme: 1, light: { bg: "#ffffff" }, dark: { bg: "#000000" } }).theme;
  assert.equal(both.name, "Imported theme");
  assert.deepEqual(Object.keys(both), ["name", "light", "dark"]);
  assert.equal(read({ platypus_theme: 1, name: "x".repeat(100), light: {} }).theme.name.length, 60);
  const bad = read({ platypus_theme: 1, light: [1], dark: { bg: "#000000" } });
  assert.deepEqual(bad.skipped, ["light: not a set of colors"]);
  assert.equal(bad.theme.light, undefined);
});

test("what is not a theme file is refused", () => {
  assert.throws(() => read("{nope"), /not JSON/);
  assert.throws(() => read("[1]"), /not a theme/);
  assert.throws(() => read({ name: "x", dark: { bg: "#000000" } }), /platypus_theme is missing/);
  assert.throws(() => read({ platypus_theme: 2, dark: { bg: "#000000" } }), /newer PlatypusDB/);
  assert.throws(() => read({ platypus_theme: 1, name: "x" }), /neither light nor dark/);
});

test("a written file reads back the same", () => {
  const theme = { name: "Sea & Sand", light: { bg: "#fafafa" }, dark: { bg: "#101010", accent: "#3fc1c9" } };
  const text = themeFileText(theme);
  assert.ok(text.startsWith('{\n  "platypus_theme": 1,'), text);
  assert.deepEqual(read(text), { theme, skipped: [] });
  assert.equal(themeFileName(theme), "sea-sand.platypus-theme.json");
  assert.equal(themeFileName({ name: "!!!" }), "theme.platypus-theme.json");
});
