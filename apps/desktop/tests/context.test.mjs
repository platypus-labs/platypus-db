// Unit tests of where the editor's session goes after USE (src/editor/context.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { setsSchema, usedDatabase } from "../src/editor/context.ts";

test("USE names the database the session switches to", () => {
  assert.equal(usedDatabase("use admin", "mongo"), "admin");
  assert.equal(usedDatabase("USE `my db`;", "mysql"), "my db");
  assert.equal(usedDatabase("  USE [master]", "sqlserver"), "master");
  assert.equal(usedDatabase('USE "x"', "sqlserver"), "x");
  assert.equal(usedDatabase("USE other ;", "mysql"), "other");
});

test("other statements and other databases switch nothing", () => {
  assert.equal(usedDatabase("use admin", "postgres"), null);
  assert.equal(usedDatabase("SELECT 'use x'", "mysql"), null);
  assert.equal(usedDatabase("USE a; SELECT 1", "mysql"), null);
  assert.equal(usedDatabase("users", "mongo"), null);
});

test("search_path changes the schema", () => {
  assert.ok(setsSchema("SET search_path = demo"));
  assert.ok(setsSchema("set local search_path to a, b"));
  assert.ok(setsSchema("SET SCHEMA 'demo'"));
  assert.ok(!setsSchema("SELECT set_config('search_path', 'x', false)"));
});
