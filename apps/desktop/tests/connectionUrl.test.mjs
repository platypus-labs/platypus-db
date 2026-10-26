// Unit tests of what a pasted connection URL does to the connection form
// (src/connectionUrl.ts); the parsing itself is the backend's.
import assert from "node:assert/strict";
import { test } from "node:test";
import { hasUrl, looksLikeConnection, withParsed } from "../src/connectionUrl.ts";

const form = (over = {}) => ({
  id: "c1",
  name: "",
  driver: "postgres",
  host: "localhost",
  port: 5432,
  database: "",
  user: "",
  ssl_mode: "prefer",
  ssl_root_cert: null,
  save_password: true,
  read_only: false,
  color: null,
  ...over,
});

const parsed = (over = {}) => ({
  driver: "postgres",
  host: "db.example",
  port: 6543,
  database: "shop",
  user: "u",
  password: "p",
  ssl_mode: "require",
  ssl_root_cert: null,
  ssl_client_cert: null,
  ssl_client_key: null,
  options: {},
  notes: [],
  ...over,
});

test("URLs, JDBC URLs and key=value strings are read as connections", () => {
  for (const text of [
    "postgres://u:p@db.example:6543/shop?sslmode=require",
    "  mongodb+srv://u@cluster0/app ",
    "jdbc:sqlserver://h;databaseName=x",
    "Server=tcp:h,1433;Database=x;User Id=sa",
    "Host=h;Username=app;",
  ]) {
    assert.equal(looksLikeConnection(text), true, text);
  }
  for (const text of ["", "My database", "db.example.com", "a=b", "line one\nServer=h;Database=x"]) {
    assert.equal(looksLikeConnection(text), false, text);
  }
});

test("the parsed fields fill the form and name it after the database", () => {
  const next = withParsed(form({ read_only: true, options: { keep: "1" } }), parsed({ options: { more: "2" } }));
  assert.equal(next.host, "db.example");
  assert.equal(next.port, 6543);
  assert.equal(next.database, "shop");
  assert.equal(next.user, "u");
  assert.equal(next.ssl_mode, "require");
  assert.equal(next.name, "shop");
  assert.equal(next.read_only, true);
  assert.deepEqual(next.options, { keep: "1", more: "2" });
  assert.equal("password" in next, false);
  // A name already typed stays; without a database the host names it.
  assert.equal(withParsed(form({ name: "Mine" }), parsed()).name, "Mine");
  assert.equal(withParsed(form(), parsed({ database: "" })).name, "db.example");
  assert.equal(withParsed(form(), parsed({ driver: "mongo", database: "app?srv=true" })).name, "app");
  assert.equal(withParsed(form(), parsed({ driver: "sqlite", host: "", database: "C:\\data\\app.db" })).name, "app.db");
});

test(".pgpass stays PostgreSQL's", () => {
  const pg = form({ pgpass: true, save_password: false });
  assert.equal(withParsed(pg, parsed()).pgpass, true);
  const my = withParsed(pg, parsed({ driver: "mysql" }));
  assert.equal(my.pgpass, false);
  assert.equal(my.save_password, true);
});

test("only drivers with a URL scheme are copied as URL", () => {
  assert.equal(hasUrl("postgres"), true);
  assert.equal(hasUrl("mongo"), true);
  assert.equal(hasUrl("sqlite"), true);
  assert.equal(hasUrl("snowflake"), false);
  assert.equal(hasUrl("oracle"), false);
  assert.equal(hasUrl("odbc"), false);
});
