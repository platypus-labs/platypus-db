// A plugin's settings fields as the Settings dialog draws them.
import assert from "node:assert/strict";
import { test } from "node:test";
import { fieldDef, fieldValue, valueToSave } from "../src/plugins/settingFields.ts";

const plugin = { id: "demo", name: "Demo" };

test("each type of field gets the control of the app's own settings", () => {
  assert.deepEqual(fieldDef(plugin, { key: "a", label: "A", type: "text" }).control, { type: "text" });
  assert.deepEqual(fieldDef(plugin, { key: "a", label: "A", type: "boolean" }).control, { type: "boolean" });
  assert.deepEqual(fieldDef(plugin, { key: "a", label: "A", type: "number", min: 1, max: 5 }).control, { type: "number", min: 1, max: 5 });
  const options = [{ value: "x", label: "X" }];
  assert.deepEqual(fieldDef(plugin, { key: "a", label: "A", type: "choice", options }).control, { type: "select", options });
});

test("a declaration names the plugin and its key", () => {
  const def = fieldDef(plugin, { key: "url", label: "URL", description: "Where", type: "text", default: "https://x" });
  assert.equal(def.key, "demo.url");
  assert.equal(def.section, "Demo");
  assert.equal(def.label, "URL");
  assert.equal(def.description, "Where");
  assert.equal(def.default, "https://x");
});

test("without a default the control starts empty", () => {
  assert.equal(fieldDef(plugin, { key: "a", label: "A", type: "text" }).default, "");
  assert.equal(fieldDef(plugin, { key: "a", label: "A", type: "boolean" }).default, false);
  assert.equal(fieldDef(plugin, { key: "a", label: "A", type: "number" }).default, 0);
  assert.equal(fieldDef(plugin, { key: "a", label: "A", type: "number", min: 2 }).default, 2);
  assert.equal(fieldDef(plugin, { key: "a", label: "A", type: "choice", options: [{ value: "x", label: "X" }] }).default, "x");
});

test("a control shows what is saved, else the default, and a false or 0 stays", () => {
  const number = { key: "n", label: "N", type: "number", default: 3 };
  assert.equal(fieldValue(number, undefined), 3);
  assert.equal(fieldValue(number, { key: "n", value: null, set: false }), 3);
  assert.equal(fieldValue(number, { key: "n", value: 0, set: true }), 0);
  const flag = { key: "b", label: "B", type: "boolean", default: true };
  assert.equal(fieldValue(flag, { key: "b", value: false, set: true }), false);
});

test("the default is not stored, and Reset removes the value", () => {
  const def = fieldDef(plugin, { key: "n", label: "N", type: "number", default: 3 });
  assert.equal(valueToSave(def, 3), null);
  assert.equal(valueToSave(def, 4), 4);
  assert.equal(valueToSave(def, null), null);
});
