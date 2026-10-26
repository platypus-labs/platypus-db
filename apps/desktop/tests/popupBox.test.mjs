// Unit tests of where a popup cell editor goes (src/grid/edit/popupBox.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { popupBox } from "../src/grid/edit/popupBox.ts";

const size = { width: 440, height: 300 };
const min = { width: 280, height: 160 };

test("the popup opens at the cell when the grid has room", () => {
  assert.deepEqual(popupBox({ left: 10, top: 20 }, { width: 1000, height: 800 }, size, min), {
    left: 10,
    top: 20,
    width: 440,
    height: 300,
  });
});

test("near the grid's edges the popup moves back inside it", () => {
  assert.deepEqual(popupBox({ left: 900, top: 700 }, { width: 1000, height: 800 }, size, min), {
    left: 556,
    top: 496,
    width: 440,
    height: 300,
  });
});

test("a grid smaller than the popup shrinks it, so that its actions stay in view", () => {
  assert.deepEqual(popupBox({ left: 50, top: 120 }, { width: 360, height: 220 }, size, min), {
    left: 4,
    top: 4,
    width: 352,
    height: 212,
  });
});

test("the popup never shrinks below its least size", () => {
  const box = popupBox({ left: 0, top: 0 }, { width: 100, height: 50 }, size, min);
  assert.equal(box.width, 280);
  assert.equal(box.height, 160);
  assert.equal(box.left, 0);
  assert.equal(box.top, 0);
});
