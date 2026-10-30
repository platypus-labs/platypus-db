// Unit tests of where a project dragged in the rail lands (railDrop in src/projects/state.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { railDrop, railOrder } from "../src/projects/state.ts";

const projects = ["a", "b", "c", "d", "e"].map((id) => ({ id }));
// The rail: a, [F: b, c], d, e.
const folders = [{ id: "F", projects: ["b", "c"] }];
const drop = (dragged, spot, fs = folders) => railDrop(projects, fs, dragged, spot);
const spot = (kind, id, zone) => ({ kind, id, zone });

test("the rail's order shows a folder where its first project is", () => {
  assert.deepEqual(railOrder(projects, folders).map((p) => p.id), ["a", "b", "c", "d", "e"]);
});

test("a loose project moves before or after another", () => {
  assert.deepEqual(drop("e", spot("project", "a", "before")), { order: ["e", "a", "b", "c", "d"] });
  assert.deepEqual(drop("a", spot("project", "e", "after")), { order: ["b", "c", "d", "e", "a"] });
  assert.deepEqual(drop("a", spot("project", "d", "before")), { order: ["b", "c", "a", "d", "e"] });
});

test("a drop where the project is already changes nothing", () => {
  assert.equal(drop("d", spot("project", "e", "before")), null);
  assert.equal(drop("d", spot("project", "d", "after")), null);
  assert.equal(drop("e", { kind: "end" }), null);
  assert.equal(drop("a", { kind: "start" }), null);
  assert.equal(drop("b", spot("folder", "F", "inside")), null, "into its own folder");
  assert.equal(drop("b", spot("project", "c", "inside")), null, "onto one of its own folder");
  assert.equal(drop("b", spot("project", "c", "before")), null);
});

test("inside an open folder a project moves among the folder's", () => {
  assert.deepEqual(drop("c", spot("project", "b", "before")), { folder: { id: "F", index: 0 }, order: ["a", "c", "b", "d", "e"] });
  assert.deepEqual(drop("b", spot("project", "c", "after")), { folder: { id: "F", index: 1 }, order: ["a", "c", "b", "d", "e"] });
});

test("a project dropped next to one in a folder joins the folder there", () => {
  assert.deepEqual(drop("e", spot("project", "c", "before")), { folder: { id: "F", index: 1 }, order: ["a", "b", "e", "c", "d"] });
  assert.deepEqual(drop("a", spot("project", "c", "after")), { folder: { id: "F", index: 2 }, order: ["b", "c", "a", "d", "e"] });
});

test("a project dropped next to a loose one leaves its folder", () => {
  assert.deepEqual(drop("b", spot("project", "e", "after")), { folder: { id: null, index: null }, order: ["a", "c", "d", "e", "b"] });
  assert.deepEqual(drop("c", spot("project", "a", "before")), { folder: { id: null, index: null }, order: ["c", "a", "b", "d", "e"] });
});

test("onto a project: a new folder of the two where the target is, or the target's folder", () => {
  assert.deepEqual(drop("e", spot("project", "a", "inside")), { group: "a", order: ["a", "e", "b", "c", "d"] });
  assert.deepEqual(drop("a", spot("project", "d", "inside")), { group: "d", order: ["b", "c", "d", "a", "e"] });
  assert.deepEqual(drop("e", spot("project", "b", "inside")), { group: "b", order: ["a", "b", "c", "e", "d"] });
});

test("onto a folder: last in it; before or after it: out of any folder, next to it", () => {
  assert.deepEqual(drop("e", spot("folder", "F", "inside")), { folder: { id: "F", index: null }, order: ["a", "b", "c", "e", "d"] });
  assert.deepEqual(drop("e", spot("folder", "F", "before")), { order: ["a", "e", "b", "c", "d"] });
  assert.deepEqual(drop("a", spot("folder", "F", "after")), { order: ["b", "c", "a", "d", "e"] });
  assert.deepEqual(drop("c", spot("folder", "F", "before")), { folder: { id: null, index: null }, order: ["a", "c", "b", "d", "e"] });
  assert.equal(drop("a", spot("folder", "F", "before")), null);
  assert.equal(drop("x", spot("folder", "G", "inside")), null, "a folder that is gone");
});

test("the ends of the rail take a project out of its folder", () => {
  assert.deepEqual(drop("b", { kind: "end" }), { folder: { id: null, index: null }, order: ["a", "c", "d", "e", "b"] });
  assert.deepEqual(drop("c", { kind: "start" }), { folder: { id: null, index: null }, order: ["c", "a", "b", "d", "e"] });
  assert.deepEqual(drop("d", { kind: "start" }), { order: ["d", "a", "b", "c", "e"] });
});

test("a folder of the dragged project alone: before or after it leaves the folder in place", () => {
  const alone = [{ id: "G", projects: ["d"] }];
  assert.deepEqual(drop("d", spot("folder", "G", "before"), alone), { folder: { id: null, index: null }, order: ["a", "b", "c", "d", "e"] });
});

test("the rail's order lists every project once, also those placed after their folder", () => {
  // e is in F but saved after d: the folder still stands where b is.
  const fs = [{ id: "F", projects: ["b", "e"] }];
  assert.deepEqual(drop("d", spot("project", "a", "before"), fs), { order: ["d", "a", "b", "e", "c"] });
});
