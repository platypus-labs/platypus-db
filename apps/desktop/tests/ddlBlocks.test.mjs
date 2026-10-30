// Unit tests of how "Generate DDL" reads many objects (src/objects/ddlBlocks.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { BLOCK, FIRST_BLOCK, blockSize, ddlText } from "../src/objects/ddlBlocks.ts";

test("a small first block, then larger ones", () => {
  assert.equal(blockSize(0), FIRST_BLOCK);
  assert.equal(blockSize(FIRST_BLOCK), BLOCK);
  assert.ok(FIRST_BLOCK < BLOCK);
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
