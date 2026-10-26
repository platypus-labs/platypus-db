// Unit tests of where a connection or folder dragged in the navigator lands
// (src/projects/arrange.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { dropChange, dropZone, listLayout, stepChange } from "../src/projects/arrange.ts";

const conn = (id) => ({ kind: "connection", id });
const folder = (id) => ({ kind: "folder", id });

// Root: folders F, G (H inside F); connections a, b at the root, c, d in F.
const layout = {
  connections: ["a", "c", "b", "d"],
  folders: [
    { id: "F", name: "F" },
    { id: "G", name: "G" },
    { id: "H", name: "H", parent: "F" },
  ],
  placement: { c: "F", d: "F" },
};

test("the zone: halves without grouping, quarters with it, onto a row of the other kind", () => {
  assert.equal(dropZone(5, 28, "connection", "connection", false), "before");
  assert.equal(dropZone(20, 28, "connection", "connection", false), "after");
  assert.equal(dropZone(5, 28, "connection", "connection", true), "before");
  assert.equal(dropZone(14, 28, "connection", "connection", true), "into");
  assert.equal(dropZone(25, 28, "connection", "connection", true), "after");
  assert.equal(dropZone(14, 28, "folder", "folder", false), "into");
  assert.equal(dropZone(2, 28, "connection", "folder", true), "into");
  assert.equal(dropZone(2, 28, "folder", "connection", true), "into");
});

test("a connection above or below another goes into its folder, next to it", () => {
  assert.deepEqual(dropChange(layout, conn("b"), conn("a"), "before"), { kind: "connection", id: "b", folder: null, before: "a" });
  assert.deepEqual(dropChange(layout, conn("a"), conn("b"), "after"), { kind: "connection", id: "a", folder: null, before: null });
  assert.deepEqual(dropChange(layout, conn("a"), conn("c"), "after"), { kind: "connection", id: "a", folder: "F", before: "d" });
  assert.deepEqual(dropChange(layout, conn("d"), conn("c"), "before"), { kind: "connection", id: "d", folder: "F", before: "c" });
});

test("a drop where the connection is already changes nothing", () => {
  assert.equal(dropChange(layout, conn("a"), conn("b"), "before"), null);
  assert.equal(dropChange(layout, conn("b"), conn("a"), "after"), null);
  assert.equal(dropChange(layout, conn("a"), conn("a"), "before"), null);
  assert.equal(dropChange(layout, conn("a"), conn("a"), "into"), null);
  assert.equal(dropChange(layout, conn("c"), folder("F"), "into"), null);
  assert.equal(dropChange(layout, conn("b"), null, "into"), null);
});

test("a connection onto another makes a folder; onto a folder goes into it", () => {
  assert.deepEqual(dropChange(layout, conn("b"), conn("c"), "into"), { kind: "group", target: "c", dragged: "b" });
  assert.deepEqual(dropChange(layout, conn("a"), folder("H"), "into"), { kind: "connection", id: "a", folder: "H", before: null });
  // Below the rows: last at the root.
  assert.deepEqual(dropChange(layout, conn("c"), null, "into"), { kind: "connection", id: "c", folder: null, before: null });
});

test("folders: reordered among their siblings, nested onto another, never into themselves", () => {
  assert.deepEqual(dropChange(layout, folder("G"), folder("F"), "before"), { kind: "folder", id: "G", parent: null, before: "F" });
  assert.equal(dropChange(layout, folder("G"), folder("F"), "after"), null);
  assert.deepEqual(dropChange(layout, folder("G"), folder("F"), "into"), { kind: "folder", id: "G", parent: "F", before: null });
  assert.deepEqual(dropChange(layout, folder("H"), folder("G"), "after"), { kind: "folder", id: "H", parent: null, before: null });
  assert.equal(dropChange(layout, folder("F"), folder("H"), "into"), null);
  assert.equal(dropChange(layout, folder("F"), folder("F"), "before"), null);
  assert.equal(dropChange(layout, folder("H"), folder("F"), "into"), null);
  // Onto a connection: where the connection is.
  assert.deepEqual(dropChange(layout, folder("G"), conn("c"), "into"), { kind: "folder", id: "G", parent: "F", before: null });
  assert.equal(dropChange(layout, folder("F"), conn("c"), "into"), null);
  assert.deepEqual(dropChange(layout, folder("H"), null, "into"), { kind: "folder", id: "H", parent: null, before: null });
});

test("Alt+Up and Alt+Down swap with the sibling above or below, not past the edge", () => {
  assert.deepEqual(stepChange(layout, conn("b"), -1), { kind: "connection", id: "b", folder: null, before: "a" });
  assert.equal(stepChange(layout, conn("a"), -1), null);
  assert.deepEqual(stepChange(layout, conn("a"), 1), { kind: "connection", id: "a", folder: null, before: null });
  assert.equal(stepChange(layout, conn("b"), 1), null);
  assert.deepEqual(stepChange(layout, conn("c"), 1), { kind: "connection", id: "c", folder: "F", before: null });
  assert.deepEqual(stepChange(layout, folder("F"), 1), { kind: "folder", id: "F", parent: null, before: null });
  assert.deepEqual(stepChange(layout, folder("G"), -1), { kind: "folder", id: "G", parent: null, before: "F" });
  assert.equal(stepChange(layout, folder("H"), 1), null);
});

test("All connections: the saved list, without folders", () => {
  const all = listLayout([{ id: "x" }, { id: "y" }, { id: "z" }]);
  assert.deepEqual(stepChange(all, conn("x"), 1), { kind: "connection", id: "x", folder: null, before: "z" });
  assert.deepEqual(dropChange(all, conn("z"), conn("x"), "before"), { kind: "connection", id: "z", folder: null, before: "x" });
});
