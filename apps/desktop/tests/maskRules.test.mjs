// Unit tests of the columns masked for screen sharing (src/guards/maskRules.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { DEFAULT_NAMES, masksColumn, patterns, isSensitive } from "../src/guards/maskRules.ts";

const masks = (name, db_type, names = DEFAULT_NAMES, types = "") => masksColumn({ name, db_type }, patterns(names), patterns(types));

test("the default names catch the usual sensitive columns, whatever their case", () => {
  for (const name of ["email", "customer_email", "Password_Hash", "api_key", "PHONE", "iban", "date_of_birth"]) assert.ok(masks(name, "text"), name);
  for (const name of ["id", "name", "created_at", "amount"]) assert.ok(!masks(name, "text"), name);
});

test("patterns match whole names, with * for any characters, and types without their size", () => {
  assert.ok(masks("note", "text", "note"));
  assert.ok(!masks("notes", "text", "note"));
  assert.ok(masks("x", "varchar(20)", "", "varchar"));
  assert.ok(masks("addr", "inet", "", "inet, cidr"));
  assert.ok(!masks("addr", "integer", "", "int"));
  assert.ok(masks("a.b", "text", "a.b"), "a dot is a dot");
  assert.ok(!masks("axb", "text", "a.b"));
});

test("an empty list masks nothing", () => {
  assert.deepEqual(patterns(" , "), []);
  assert.ok(!masks("email", "text", "", ""));
});

test("the user's mark on a column counts over the rules, both ways", () => {
  const names = patterns(DEFAULT_NAMES);
  const email = { name: "email", db_type: "text" };
  const note = { name: "note", db_type: "text" };
  assert.equal(isSensitive(undefined, email, names, []), true, "the rules say so");
  assert.equal(isSensitive(null, note, names, []), false);
  assert.equal(isSensitive(false, email, names, []), false, "marked not sensitive");
  assert.equal(isSensitive(true, note, names, []), true, "marked sensitive");
});
