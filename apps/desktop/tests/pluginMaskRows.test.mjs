// What a plugin's grid action reads of the selected rows (src/plugins/maskRows.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { maskRows } from "../src/plugins/maskRows.ts";

test("a sensitive column reads masked, whatever its type, and null stays null", () => {
  const rows = [["Ann", "s3cret", 1], ["Bob", null, 2], ["Cy", 4242, 3]];
  assert.deepEqual(maskRows(rows, [false, true, false], "••"), [["Ann", "••", 1], ["Bob", null, 2], ["Cy", "••", 3]]);
});

test("with nothing sensitive the rows are the same ones", () => {
  const rows = [["Ann"]];
  assert.equal(maskRows(rows, [false], "••"), rows);
});
