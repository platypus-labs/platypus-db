// Unit tests of the pure parts of the pointer drags inside the page (src/ui/pointerDrag.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { DRAG_THRESHOLD, edgeZone, markedTarget, pastThreshold, startsDrag } from "../src/ui/pointerDrag.ts";

test("a press drags only once the pointer moves the threshold away", () => {
  const from = { x: 100, y: 100 };
  assert.equal(pastThreshold(from, from), false);
  assert.equal(pastThreshold(from, { x: 102, y: 102 }), false);
  assert.equal(pastThreshold(from, { x: 100 + DRAG_THRESHOLD, y: 100 }), true);
  assert.equal(pastThreshold(from, { x: 100, y: 100 - DRAG_THRESHOLD }), true);
  assert.equal(pastThreshold(from, { x: 103, y: 103 }), true, "diagonally, by distance");
  assert.equal(pastThreshold(from, { x: 110, y: 100 }, 20), false);
});

test("the edges of a target are before and after it, the rest inside", () => {
  assert.equal(edgeZone(0, 32), "before");
  assert.equal(edgeZone(7.9, 32), "before");
  assert.equal(edgeZone(8, 32), "inside");
  assert.equal(edgeZone(23.9, 32), "inside");
  assert.equal(edgeZone(24, 32), "after");
  assert.equal(edgeZone(32, 32), "after");
  assert.equal(edgeZone(15, 32, 0.5), "before", "halves: no inside");
  assert.equal(edgeZone(16, 32, 0.5), "after");
});

/** A fake element with attributes, under `parent`. */
const el = (attrs, parentElement = null) => ({ parentElement, getAttribute: (name) => attrs[name] ?? null });

test("the drop target is the nearest marked element at or above the one under the pointer", () => {
  const rail = el({ "data-drop": "rail" });
  const folder = el({ "data-drop": "open" }, rail);
  const square = el({ "data-drop": "project" }, folder);
  const mark = el({}, square);
  assert.deepEqual(markedTarget(mark, "data-drop"), { element: square, value: "project" });
  assert.deepEqual(markedTarget(square, "data-drop"), { element: square, value: "project" });
  assert.deepEqual(markedTarget(el({}, folder), "data-drop"), { element: folder, value: "open" });
  assert.equal(markedTarget(el({}), "data-drop"), null);
  assert.equal(markedTarget(null, "data-drop"), null);
  // An empty value marks a target too: the top of the scripts.
  const header = el({ "data-script-folder": "" });
  assert.deepEqual(markedTarget(el({}, header), "data-script-folder"), { element: header, value: "" });
});

/** A fake element `tag` with attributes, under `parentElement`. */
const node = (tagName, attrs = {}, parentElement = null) => ({ tagName, parentElement, getAttribute: (name) => attrs[name] ?? null });

test("a press on a control or an editable part inside the dragged element does not drag", () => {
  const row = node("LI");
  assert.equal(startsDrag(row, row), true);
  assert.equal(startsDrag(node("SPAN", {}, node("SPAN", {}, row)), row), true);
  assert.equal(startsDrag(node("BUTTON", {}, row), row), false);
  assert.equal(startsDrag(node("svg", {}, node("button", {}, row)), row), false, "inside a button");
  assert.equal(startsDrag(node("INPUT", {}, row), row), false);
  assert.equal(startsDrag(node("TEXTAREA", {}, row), row), false);
  assert.equal(startsDrag(node("SELECT", {}, row), row), false);
  assert.equal(startsDrag(node("DIV", { contenteditable: "true" }, row), row), false);
  assert.equal(startsDrag(node("DIV", { contenteditable: "false" }, row), row), true);
  assert.equal(startsDrag(node("SPAN", { "data-no-drag": "" }, row), row), false, "a twisty");
});

test("the dragged element may itself be a button, unless marked", () => {
  const square = node("BUTTON");
  assert.equal(startsDrag(node("I", {}, square), square), true);
  assert.equal(startsDrag(square, square), true);
  const marked = node("BUTTON", { "data-no-drag": "" });
  assert.equal(startsDrag(marked, marked), false);
});
