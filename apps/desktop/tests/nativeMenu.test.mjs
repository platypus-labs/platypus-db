// Unit tests of which right clicks keep the browser's menu (src/ui/nativeMenu.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { keepsNativeMenu } from "../src/ui/nativeMenu.ts";

test("only plain text fields keep the browser menu", () => {
  assert.equal(keepsNativeMenu({ tagName: "INPUT" }), true);
  assert.equal(keepsNativeMenu({ tagName: "INPUT", type: "search" }), true);
  assert.equal(keepsNativeMenu({ tagName: "INPUT", type: "checkbox" }), false);
  assert.equal(keepsNativeMenu({ tagName: "TEXTAREA" }), true);
  assert.equal(keepsNativeMenu({ tagName: "DIV", isContentEditable: true }), true);
  assert.equal(keepsNativeMenu({ tagName: "DIV" }), false);
  assert.equal(keepsNativeMenu(null), false);
});

test("the code editor never does", () => {
  const inEditor = { tagName: "DIV", isContentEditable: true, closest: (s) => (s === ".cm-editor" ? {} : null) };
  assert.equal(keepsNativeMenu(inEditor), false);
});
