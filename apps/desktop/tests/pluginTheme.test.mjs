// Unit tests of plugin themes (src/themes/color.ts, contrast.ts): what may reach the page's CSS, the
// list of tokens shared with the backend, the contrast warning; and what the catalog page offers
// (src/plugins/catalogEntry.ts).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { isThemeColor, luminance, pluginThemeOf, themeFontsKey } from "../src/themes/color.ts";
import { contrastRatio, lowContrast, opaqueRgb } from "../src/themes/contrast.ts";
import { entryAction, keepableSettings } from "../src/plugins/catalogEntry.ts";

const repoFile = (path) => readFileSync(new URL(`../../../${path}`, import.meta.url), "utf8");

test("a new version of a plugin counts as other fonts", () => {
  assert.notEqual(themeFontsKey("sea", "1.0.0", ["ui"]), themeFontsKey("sea", "1.1.0", ["ui"]));
  assert.equal(themeFontsKey("sea", "1.0.0", ["ui"]), themeFontsKey("sea", "1.0.0", ["ui"]));
  assert.equal(themeFontsKey("sea", "1.0.0", []), null);
});

test("a color and nothing else", () => {
  for (const good of ["#fff", "#ffff", "#0B1D26", "#0b1d26cc", "rgb(1, 2, 3)", "rgb(1 2 3 / 50%)", "rgba(255, 141, 40, 0.14)", "hsl(200, 50%, 40%)", "hsla(200, 50%, 40%, .5)", "transparent"]) {
    assert.equal(isThemeColor(good), true, good);
  }
  for (const bad of [
    "",
    "red",
    "#ggg",
    "#12345",
    " #fff",
    "#fff ",
    "#fff\n",
    "url(https://example.com/x.png)",
    "URL(x)",
    "expression(alert(1))",
    "var(--x)",
    "#fff; background: url(x)",
    "#fff}body{display:none",
    "rgb(1,2,3); color: red",
    "rgb(1,2,3)) url(x",
    "rgb(url(x))",
    "rgb()",
    "rgb(,)",
    "rgb(1,2,3",
    "rgb(1\\2c 2,3)",
    "RGB(1,2,3)",
    "color-mix(in srgb, #000 20%, #fff)",
    "/* */ #fff",
    `rgb(${"1,".repeat(40)})`,
    7,
    null,
  ]) {
    assert.equal(isThemeColor(bad), false, String(bad));
  }
});

test("the tokens of the backend are the app's palette", () => {
  const types = repoFile("apps/desktop/src/theme.ts");
  const app = [...types.slice(types.indexOf("type BasePalette"), types.indexOf("type Palette =")).matchAll(/^\s*\| "([\w-]+)",?\r?$/gm)].map((m) => m[1]);
  assert.ok(app.length > 30, "the palette was read");
  const rust = repoFile("crates/platypus-plugins/src/theme.rs");
  const list = rust.slice(rust.indexOf("pub const TOKENS"), rust.indexOf("];", rust.indexOf("pub const TOKENS")));
  const backend = [...list.matchAll(/"([\w-]+)"/g)].map((m) => m[1]);
  assert.deepEqual([...backend].sort(), [...app].sort());
});

test("only known tokens with color values are painted", () => {
  const theme = pluginThemeOf(
    {
      name: "Sea",
      dark: { bg: "#000000", accent: "url(https://example.com/x)", glow: "#ffffff" },
      light: { fg: "expression(1)" },
    },
    ["bg", "fg", "accent"],
  );
  assert.deepEqual(theme, { name: "Sea", dark: { bg: "#000000" } });
  // A theme with nothing left to paint is no theme.
  assert.equal(pluginThemeOf({ name: "x", dark: { bg: "url(x)" } }, ["bg"]), null);
  assert.equal(pluginThemeOf({ name: "x" }, ["bg"]), null);
  const both = pluginThemeOf({ name: "x", light: { bg: "#fff" }, dark: { bg: "#000" } }, ["bg"]);
  assert.deepEqual(Object.keys(both), ["name", "light", "dark"]);
});

test("luminance and contrast follow WCAG", () => {
  assert.equal(luminance(1, 1, 1), 1);
  assert.equal(luminance(0, 0, 0), 0);
  assert.equal(contrastRatio("#000000", "#ffffff"), 21);
  assert.equal(contrastRatio("#fff", "#fff"), 1);
  assert.equal(contrastRatio("rgb(0, 0, 0)", "rgb(255 255 255)"), 21);
  assert.equal(contrastRatio("rgba(0, 0, 0, 1)", "#ffffff"), 21);
  // What is translucent depends on what is behind it, so it is not told.
  assert.equal(opaqueRgb("rgba(0, 0, 0, 0.5)"), null);
  assert.equal(opaqueRgb("rgb(0 0 0 / 50%)"), null);
  assert.deepEqual(opaqueRgb("rgb(0 0 0 / 100%)"), [0, 0, 0]);
  assert.equal(opaqueRgb("#00000080"), null);
  assert.equal(opaqueRgb("hsl(0, 0%, 0%)"), null);
  assert.equal(contrastRatio("transparent", "#fff"), null);
});

test("low contrast is listed, to warn about", () => {
  const legible = { bg: "#ffffff", "bg-sidebar": "#f4f5f7", "grid-bg": "#ffffff", fg: "#111111", "fg-muted": "#555555", accent: "#0b6e78", "accent-fg": "#ffffff", "syn-keyword": "#155e75", "syn-string": "#1e7b4f" };
  assert.deepEqual(lowContrast(legible), []);
  const unreadable = lowContrast({ ...legible, fg: "#eeeeee" });
  assert.deepEqual(
    unreadable.map((p) => `${p.fg}/${p.bg}`),
    ["fg/bg", "fg/bg-sidebar", "fg/grid-bg"],
  );
  assert.equal(unreadable[0].ratio, 1.2);
  // A color it cannot tell, or one the theme lacks, is not a warning.
  assert.deepEqual(lowContrast({ bg: "#ffffff", fg: "rgba(0, 0, 0, 0.1)" }), []);
  assert.deepEqual(lowContrast({}), []);
});

test("a step back is confirmed, and only what can stay is offered at removal", () => {
  assert.deepEqual(entryAction({ status: "older" }), { confirmDowngrade: true });
  for (const status of ["new", "update", "current"]) {
    assert.deepEqual(entryAction({ status }), { confirmDowngrade: false });
  }
  assert.equal(keepableSettings(undefined), false);
  assert.equal(keepableSettings([]), false);
  assert.equal(keepableSettings([{ type: "secret" }]), false);
  assert.equal(keepableSettings([{ type: "secret" }, { type: "text" }]), true);
});
