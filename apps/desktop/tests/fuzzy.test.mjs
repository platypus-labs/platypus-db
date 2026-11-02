// Unit tests of the command palette's matching and highlight (src/shell/fuzzy.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { fuzzy, highlightRuns } from "../src/shell/fuzzy.ts";

const bold = (text, query) =>
  highlightRuns(text, fuzzy(query, text).positions)
    .filter((r) => r.marked)
    .map((r) => r.text)
    .join("");

test("the highlight follows the matched characters after an emoji", () => {
  assert.equal(bold("🦆 Ducks", "ducks"), "Ducks");
  assert.equal(bold("a🦆b🦆c", "abc"), "abc");
});

test("runs keep every character in order", () => {
  const runs = highlightRuns("Open table", fuzzy("otb", "Open table").positions);
  assert.equal(runs.map((r) => r.text).join(""), "Open table");
  assert.equal(bold("Open table", "otb"), "Otb");
});

test("a prefix of the text beats a scattered match", () => {
  assert.ok(fuzzy("or", "Order sets").score > fuzzy("or", "customers").score);
  assert.ok(fuzzy("or", "New order").score > fuzzy("or", "customers").score);
});
