// Unit tests of the admin tab's parameter list: search, "only changed" and how values read
// (src/admin/parametersModel.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { changedCount, parameterMatches, shownParameters, tellsDefaults, valueText } from "../src/admin/parametersModel.ts";

const param = (name, extra = {}) => ({
  name,
  value: "1",
  unit: null,
  default: "1",
  changed: false,
  source: "default",
  context: "user",
  description: null,
  min: null,
  max: null,
  pending_restart: false,
  statement: null,
  ...extra,
});

const list = [
  param("work_mem", { value: "8192", default: "4096", unit: "kB", changed: true, description: "Memory for sorts" }),
  param("max_connections", { value: "100", default: "100", description: "Clients at once" }),
  param("shared_buffers", { pending_restart: true, source: "configuration file" }),
];

test("every word must match, in any field, ignoring case", () => {
  assert.equal(parameterMatches(list[0], "WORK mem"), true);
  assert.equal(parameterMatches(list[0], "sorts"), true);
  assert.equal(parameterMatches(list[0], "sorts clients"), false);
  assert.equal(parameterMatches(list[2], "configuration"), true);
  assert.equal(parameterMatches(list[0], "   "), true);
});

test("only changed keeps the changed and those waiting for a restart", () => {
  assert.deepEqual(shownParameters(list, "", true).map((p) => p.name), ["work_mem", "shared_buffers"]);
  assert.deepEqual(shownParameters(list, "mem", true).map((p) => p.name), ["work_mem"]);
  assert.deepEqual(shownParameters(list, "conn", false).map((p) => p.name), ["max_connections"]);
  assert.deepEqual(shownParameters(list, "nothing", false), []);
});

test("counts the changed", () => {
  assert.equal(changedCount(list), 1);
  assert.equal(changedCount([]), 0);
});

test("a value reads with its unit", () => {
  assert.equal(valueText("8192", "kB"), "8192 kB");
  assert.equal(valueText("on", null), "on");
  assert.equal(valueText("", "kB"), "");
  assert.equal(valueText(null, "kB"), "");
});

test("defaults are told when some parameter has one", () => {
  assert.equal(tellsDefaults(list), true);
  assert.equal(tellsDefaults([param("a", { default: null })]), false);
});
