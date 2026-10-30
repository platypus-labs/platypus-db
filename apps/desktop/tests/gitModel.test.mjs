// Unit tests of what the Scripts panel derives from the Git status (src/scripts/gitModel.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { changedInFolder, conflicts, selectedPaths, statusByPath, validIdentity } from "../src/scripts/gitModel.ts";

const file = (path, badge) => ({ path, orig_path: null, badge });
const files = [file("a.sql", "M"), file("sub/b.sql", "?"), file("sub/deep/c.sql", "D"), file("x.sql", "U")];

test("changed scripts are found by path", () => {
  assert.equal(statusByPath(files).get("sub/b.sql").badge, "?");
  assert.equal(statusByPath(files).get("none.sql"), undefined);
});

test("a folder is marked when a script anywhere under it changed", () => {
  assert.equal(changedInFolder("sub", files), true);
  assert.equal(changedInFolder("sub/deep", files), true);
  assert.equal(changedInFolder("su", files), false);
  assert.equal(changedInFolder("other", files), false);
});

test("a commit takes the files that stay ticked, in order", () => {
  assert.deepEqual(selectedPaths(files, new Set()), ["a.sql", "sub/b.sql", "sub/deep/c.sql", "x.sql"]);
  assert.deepEqual(selectedPaths(files, new Set(["sub/b.sql", "x.sql"])), ["a.sql", "sub/deep/c.sql"]);
});

test("only unmerged files are conflicts", () => {
  assert.deepEqual(conflicts(files), ["x.sql"]);
  assert.deepEqual(conflicts([]), []);
});

test("an author needs a name and an email", () => {
  assert.equal(validIdentity("Ada", "ada@example.com"), true);
  assert.equal(validIdentity(" ", "ada@example.com"), false);
  assert.equal(validIdentity("Ada", "ada"), false);
  assert.equal(validIdentity("Ada", "a b@c"), false);
});
