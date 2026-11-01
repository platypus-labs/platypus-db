// Unit tests of the resizing of the query builder's boxes and the panning of the ER diagram.
import assert from "node:assert/strict";
import { test } from "node:test";
import { BOX, BOX_MIN_H, BOX_MIN_W, boxWidth, resizedBox } from "../src/builder/boxSize.ts";
import { pannedScroll } from "../src/erd/pan.ts";

test("a box without its own width has the default one", () => {
  assert.equal(boxWidth({}), BOX);
  assert.equal(boxWidth({ w: 320 }), 320);
});

test("a resized box follows the handle and never gets smaller than the minimum", () => {
  assert.deepEqual(resizedBox({ w: 200, h: 150 }, 40, -10), { w: 240, h: 140 });
  assert.deepEqual(resizedBox({ w: 200, h: 150 }, -500, -500), { w: BOX_MIN_W, h: BOX_MIN_H });
});

test("panning moves the scroll against the pointer and stops at the edge", () => {
  assert.deepEqual(pannedScroll({ left: 100, top: 50 }, { x: 10, y: 10 }, { x: 30, y: 5 }), { left: 80, top: 55 });
  assert.deepEqual(pannedScroll({ left: 10, top: 10 }, { x: 0, y: 0 }, { x: 50, y: 50 }), { left: 0, top: 0 });
});
