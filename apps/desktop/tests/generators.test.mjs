// Unit tests of the generators suggested for fake rows (src/transfer/generators.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { suggest } from "../src/transfer/generators.ts";

const column = (name, data_type, primary_key = false) => ({ name, data_type, primary_key, default: null, enum_values: [] });

test("a table's columns get generators by their keys, names and types", () => {
  assert.equal(suggest(column("id", "integer", true), undefined), "next_key");
  assert.equal(suggest(column("id", "uuid", true), undefined), "uuid");
  assert.equal(suggest(column("email", "text"), undefined, true), "email");
  assert.equal(suggest(column("code", "numeric"), undefined, true), "next_key");
  assert.equal(suggest(column("tags", "array"), undefined), "text", "no MongoDB generators");
  assert.equal(suggest(column("n", "long"), undefined), "text", "Oracle's LONG is text");
});

test("a collection's fields get generators of MongoDB's types", () => {
  const mongo = (c, unique = false) => suggest(c, undefined, unique, true);
  assert.equal(mongo(column("_id", "objectId", true), true), "object_id");
  assert.equal(mongo(column("_id", "int", true), true), "next_key");
  assert.equal(mongo(column("_id", "long", true), true), "next_key");
  assert.equal(mongo(column("email", "string"), true), "email");
  assert.equal(mongo(column("full_name", "string")), "full_name");
  assert.equal(mongo(column("country", "string")), "text");
  assert.equal(mongo(column("created_at", "date")), "date");
  assert.equal(mongo(column("tags", "array")), "word_list");
  assert.equal(mongo(column("address", "document")), "document");
  assert.equal(mongo(column("visits", "long")), "integer");
  assert.equal(mongo(column("score", "double")), "decimal");
});
