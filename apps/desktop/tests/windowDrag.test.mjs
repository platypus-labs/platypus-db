// Unit tests of the title bar's move and maximize on Linux (src/shell/windowDrag.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { DRAG_ATTR, dragHandlers } from "../src/shell/windowDrag.ts";

function fakeWindow() {
  const calls = [];
  return {
    calls,
    startDragging: async () => void calls.push("drag"),
    toggleMaximize: async () => void calls.push("maximize"),
  };
}

const part = { hasAttribute: (name) => name === DRAG_ATTR };
const control = { hasAttribute: () => false };
const at = (x, y, target = part, buttons = 1) => ({ button: 0, buttons, clientX: x, clientY: y, target });

test("a press alone does not move the window", () => {
  const win = fakeWindow();
  const h = dragHandlers(win);
  h.onMouseDown(at(10, 10));
  h.onMouseUp();
  assert.deepEqual(win.calls, []);
});

test("the move starts once the pointer leaves the press by a few pixels, once", () => {
  const win = fakeWindow();
  const h = dragHandlers(win);
  h.onMouseDown(at(10, 10));
  h.onMouseMove(at(11, 12));
  assert.deepEqual(win.calls, [], "a tremor of the hand");
  h.onMouseMove(at(14, 10));
  h.onMouseMove(at(30, 10));
  assert.deepEqual(win.calls, ["drag"]);
});

test("a double-click maximizes, without moving the window", () => {
  const win = fakeWindow();
  const h = dragHandlers(win);
  for (let i = 0; i < 2; i++) {
    h.onMouseDown(at(50, 8));
    h.onMouseUp();
  }
  h.onDoubleClick(at(50, 8));
  assert.deepEqual(win.calls, ["maximize"]);
});

test("controls on the bar neither move nor maximize the window", () => {
  const win = fakeWindow();
  const h = dragHandlers(win);
  h.onMouseDown(at(10, 10, control));
  h.onMouseMove(at(40, 10, control));
  h.onDoubleClick(at(10, 10, control));
  h.onDoubleClick(at(10, 10, null));
  assert.deepEqual(win.calls, []);
});

test("other buttons, or the button released outside the window, start nothing", () => {
  const win = fakeWindow();
  const h = dragHandlers(win);
  h.onMouseDown({ ...at(10, 10), button: 2 });
  h.onMouseMove(at(40, 10));
  h.onDoubleClick({ ...at(10, 10), button: 1 });
  // Released outside: the next move arrives with no button held.
  h.onMouseDown(at(10, 10));
  h.onMouseMove(at(40, 10, part, 0));
  h.onMouseMove(at(80, 10));
  assert.deepEqual(win.calls, []);
});

test("a failed call goes to the error handler", async () => {
  const errors = [];
  const h = dragHandlers(
    { startDragging: () => Promise.reject(new Error("no")), toggleMaximize: () => Promise.reject(new Error("no")) },
    (e) => errors.push(e.message),
  );
  h.onDoubleClick(at(5, 5));
  h.onMouseDown(at(5, 5));
  h.onMouseMove(at(50, 5));
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual(errors, ["no", "no"]);
});
