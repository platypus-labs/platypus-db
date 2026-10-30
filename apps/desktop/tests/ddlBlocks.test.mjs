// Unit tests of how "Generate DDL" reads many objects (src/objects/ddlBlocks.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { BLOCK, FIRST_BLOCK, appended, ddlBlocks, ddlText } from "../src/objects/ddlBlocks.ts";

test("a small first block, then larger ones, covering every object once", () => {
  assert.deepEqual(ddlBlocks(0), []);
  assert.deepEqual(ddlBlocks(1), [[0, 1]]);
  assert.deepEqual(ddlBlocks(FIRST_BLOCK), [[0, FIRST_BLOCK]]);
  const blocks = ddlBlocks(80);
  assert.deepEqual(blocks[0], [0, FIRST_BLOCK]);
  assert.deepEqual(blocks[1], [FIRST_BLOCK, FIRST_BLOCK + BLOCK]);
  assert.equal(blocks.at(-1)[1], 80);
  for (let i = 1; i < blocks.length; i++) assert.equal(blocks[i][0], blocks[i - 1][1], "no gap nor overlap");
});

test("an object without DDL becomes a comment with why, on one line", () => {
  const objects = [
    { schema: "s", name: "a", kind: "table", parent: null },
    { schema: "s", name: "b", kind: "view", parent: null },
  ];
  const text = ddlText(objects, [{ ddl: "CREATE TABLE a ();" }, { error: "permission\ndenied" }]);
  assert.equal(text, "CREATE TABLE a ();\n\n-- s.b: permission denied");
  assert.equal(ddlText(objects.slice(0, 1), []), "-- s.a: no answer");
});

test("blocks are separated by a blank line, the first one goes in as it is", () => {
  assert.equal(appended("", "A"), "A");
  assert.equal(appended("  \n", "A"), "A");
  assert.equal(appended("X", "A"), "\n\nA");
});
