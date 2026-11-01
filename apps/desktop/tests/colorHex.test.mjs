// Unit tests of the CSS color to #rrggbb conversion (src/ui/colorHex.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { toHex } from "../src/ui/colorHex.ts";

test("hex colors normalize, rgb() converts, the rest is null", () => {
  assert.equal(toHex(" #1A2B3C "), "#1a2b3c");
  assert.equal(toHex("#abc"), "#aabbcc");
  assert.equal(toHex("rgb(255, 0, 16)"), "#ff0010");
  assert.equal(toHex("rgba(0 128 255 / 50%)"), "#0080ff");
  assert.equal(toHex("color-mix(in srgb, red 50%, blue)"), null);
  assert.equal(toHex(""), null);
});
