// Unit tests of the submenu placement (src/components/menuPlacement.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { submenuPlacement } from "../src/components/menuPlacement.ts";

const win = { width: 1280, height: 720 };
const size = { width: 200, height: 300 };

test("fits: opens to the right, a little above the row", () => {
  assert.deepEqual(submenuPlacement({ left: 100, top: 100, width: 180 }, size, win), { left: 176, top: -5 });
});

test("near the bottom: shifted up so it ends inside the window", () => {
  const p = submenuPlacement({ left: 100, top: 600, width: 180 }, size, win);
  assert.equal(600 + p.top + size.height, win.height - 4);
});

test("near the right edge: flips to the left of the anchor", () => {
  const p = submenuPlacement({ left: 1000, top: 100, width: 180 }, size, win);
  assert.equal(1000 + p.left + size.width, 1004);
});

test("taller than the window: max-height and pinned to the top", () => {
  const p = submenuPlacement({ left: 100, top: 400, width: 180 }, { width: 200, height: 900 }, win);
  assert.equal(p.maxHeight, 712);
  assert.equal(400 + p.top, 4);
});
