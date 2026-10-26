// Unit tests of Enter after a word typed in full (src/editor/typedWord.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { typedInFull, wordBefore } from "../src/editor/typedWord.ts";

test("a word typed exactly as the entry, which inserts nothing more, is a newline", () => {
  assert.equal(wordBefore("SELECT * FROM orders"), "orders");
  assert.equal(typedInFull("orders", { label: "orders" }), true);
  assert.equal(typedInFull("FROM", { label: "FROM", type: "keyword" }), true);
});

test("a word with letters beyond ASCII is a word too", () => {
  assert.equal(wordBefore("SELECT * FROM città"), "città");
  assert.equal(typedInFull(wordBefore("WHERE straße"), { label: "straße" }), true);
  assert.equal(wordBefore("SELECT 東京_2"), "東京_2");
  // A combining accent (decomposed text) stays in the word.
  assert.equal(wordBefore("SELECT città"), "città");
  assert.equal(wordBefore("SELECT a + "), undefined);
});

test("Enter accepts when the entry would change what was typed", () => {
  // Postgres column `Email`, which must be quoted.
  assert.equal(typedInFull("email", { label: "Email", apply: '"Email"' }), false);
  assert.equal(typedInFull("Email", { label: "Email", apply: '"Email"' }), false);
  // Keyword case: `select` becomes `SELECT`.
  assert.equal(typedInFull("select", { label: "SELECT" }), false);
  // Mongo `find` gives `find()`, `use` gives `use `; snippets apply a function.
  assert.equal(typedInFull("find", { label: "find", apply: "find()" }), false);
  assert.equal(typedInFull("use", { label: "use", apply: "use " }), false);
  assert.equal(typedInFull("count", { label: "count", apply: () => {} }), false);
  assert.equal(typedInFull(undefined, { label: "x" }), false);
  assert.equal(typedInFull("x", null), false);
});
