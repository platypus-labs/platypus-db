// Unit tests of the generators suggested for fake rows (src/transfer/generators.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { numberRange, REPEATING, suggest } from "../src/transfer/generators.ts";

const column = (name, data_type, primary_key = false, nullable = true) => ({ name, data_type, primary_key, nullable, default: null, enum_values: [] });

test("a table's columns get generators by their keys, names and types", () => {
  assert.equal(suggest(column("id", "integer", true), undefined), "next_key");
  assert.equal(suggest(column("id", "uuid", true), undefined), "uuid");
  assert.equal(suggest(column("email", "text"), undefined, true), "email");
  assert.equal(suggest(column("code", "numeric"), undefined, true), "next_key");
  assert.equal(suggest(column("tags", "array"), undefined), "null", "no MongoDB generators");
  assert.equal(suggest(column("n", "long"), undefined), "text", "Oracle's LONG is text");
});

test("columns get values their types take, or are left out", () => {
  const kind = (name, type, nullable = true) => suggest(column(name, type, false, nullable), undefined);
  assert.equal(kind("tags", "text[]"), "null", "a PostgreSQL array");
  assert.equal(kind("tags", "Array(String)", false), "skip", "ClickHouse fills its default");
  assert.equal(kind("meta", "jsonb"), "document");
  assert.equal(kind("ip", "inet"), "ip");
  assert.equal(kind("t", "time without time zone"), "time");
  assert.equal(kind("iv", "interval"), "null", "an interval holds no int");
  assert.equal(kind("flag", "tinyint(1)"), "bool", "MySQL's BOOLEAN");
  assert.equal(kind("tiny", "tinyint"), "integer");
  assert.equal(kind("name_id", "integer"), "integer", "names only for text");
  assert.equal(kind("shape", "geometry", false), "skip");
  // MariaDB writes no default as the text NULL.
  assert.equal(suggest({ ...column("name", "varchar(50)"), default: "NULL" }, undefined), "full_name");
  assert.equal(suggest({ ...column("n", "int"), default: "0" }, undefined), "skip");
  // Identities and computed columns are the database's.
  assert.equal(suggest(column("id", "int", true), undefined, false, false, true), "skip");
});

test("numbers within what the type holds", () => {
  assert.deepEqual(numberRange("tinyint"), { min: 0, max: 100, scale: 0 });
  assert.deepEqual(numberRange("numeric(5,2)"), { min: 0, max: 999, scale: 2 });
  assert.deepEqual(numberRange("numeric(3)"), { min: 0, max: 999, scale: 0 });
  assert.deepEqual(numberRange("year"), { min: 1990, max: 2030, scale: 0 });
  assert.deepEqual(numberRange("integer"), { min: 1, max: 1000, scale: 2 });
});

test("a collection's fields get generators of MongoDB's types", () => {
  const mongo = (c, unique = false) => suggest(c, undefined, unique, true);
  assert.equal(mongo(column("_id", "objectId", true), true), "object_id");
  assert.equal(mongo(column("_id", "int", true), true), "next_key");
  assert.equal(mongo(column("_id", "long", true), true), "next_key");
  assert.equal(mongo(column("email", "string"), true), "email");
  assert.equal(mongo(column("full_name", "string")), "full_name");
  assert.equal(mongo(column("country", "string")), "text");
  assert.equal(mongo(column("created_at", "date")), "timestamp", "BSON dates hold the time");
  assert.equal(suggest(column("born", "date"), undefined), "date", "a SQL date has none");
  // An _id that is not a number gets ObjectIds, not numbers among its keys.
  assert.equal(mongo(column("_id", "string", true), true), "object_id");
  assert.equal(mongo(column("_id", "mixed", true), true), "object_id");
  assert.equal(mongo(column("_id", "binData", true), true), "object_id");
  assert.equal(mongo(column("_id", "double", true), true), "next_key");
  assert.equal(mongo(column("_id", "decimal", true), true), "next_key");
  assert.equal(mongo(column("tags", "array")), "word_list");
  assert.equal(mongo(column("address", "document")), "document");
  assert.equal(mongo(column("visits", "long")), "integer");
  assert.equal(mongo(column("score", "double")), "decimal");
});

test("a unique field is not offered arrays or documents, which repeat", () => {
  for (const kind of ["word_list", "document", "integer", "date"]) assert.ok(REPEATING.includes(kind), kind);
  for (const kind of ["object_id", "next_key", "email", "uuid"]) assert.ok(!REPEATING.includes(kind), kind);
});
