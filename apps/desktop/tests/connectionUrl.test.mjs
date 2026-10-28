// Unit tests of what a pasted connection URL does to the connection form
// (src/connectionUrl.ts); the parsing itself is the backend's.
import assert from "node:assert/strict";
import { test } from "node:test";
import { applyUrlEdit, hasUrl, looksLikeConnection, splitLogin, urlModeDriver, validPort, withParsed } from "../src/connectionUrl.ts";

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
    "host=h port=5432 dbname=d user=u password=x sslmode=require",
    "password='a b' host=h",
  ]) {
    assert.equal(looksLikeConnection(text), true, text);
  }
  for (const text of ["", "My database", "db.example.com", "a=b", "Sales=EU;", "x = y", "line one\nServer=h;Database=x"]) {
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

test("a login typed in the host field is split into its fields", () => {
  assert.deepEqual(splitLogin("root:s3cret@db.example.com:3306"), { user: "root", password: "s3cret", host: "db.example.com", port: 3306 });
  assert.deepEqual(splitLogin(" me@db "), { user: "me", password: null, host: "db", port: null });
  // The password may hold @ and :, the host is after the last @.
  assert.deepEqual(splitLogin("u:p@ss:w@h"), { user: "u", password: "p@ss:w", host: "h", port: null });
  assert.deepEqual(splitLogin("u:p@[::1]:5432"), { user: "u", password: "p", host: "::1", port: 5432 });
  assert.deepEqual(splitLogin("u@[fe80::1]"), { user: "u", password: null, host: "fe80::1", port: null });
  for (const plain of ["db.example.com", "@h", "u@", "postgres://u:p@h/db", "u@a:1,b:2", "u@h:99999", "u@h/db"]) {
    assert.equal(splitLogin(plain), null, plain);
  }
});

test("ports are 1 to 65535, or none", () => {
  assert.equal(validPort(null), true);
  assert.equal(validPort(5432), true);
  assert.equal(validPort(65535), true);
  for (const bad of [0, 70000, -1, 1.5, Number.NaN]) assert.equal(validPort(bad), false, String(bad));
});

test("the URL mode is for the drivers with a server and a URL", () => {
  for (const d of ["postgres", "mysql", "sqlserver", "clickhouse", "libsql", "mongo"]) assert.equal(urlModeDriver(d), true, d);
  for (const d of ["sqlite", "duckdb", "oracle", "snowflake", "odbc"]) assert.equal(urlModeDriver(d), false, d);
});

test("a URL edit updates the settings but keeps the name and the driver", () => {
  const out = applyUrlEdit(form({ name: "Mine" }), "old", "postgres://u@db.example:6543/shop", parsed({ password: null, driver: "mysql" }));
  assert.equal(out.conn.host, "db.example");
  assert.equal(out.conn.port, 6543);
  assert.equal(out.conn.database, "shop");
  assert.equal(out.conn.name, "Mine");
  assert.equal(out.conn.driver, "postgres");
  assert.equal(out.password, "old");
  assert.equal(out.draft, "postgres://u@db.example:6543/shop");
});

test("a password in the URL goes to the password field and out of the URL text", () => {
  const out = applyUrlEdit(form(), "", "postgres://u:p@db.example/shop", parsed({ password: "p" }));
  assert.equal(out.password, "p");
  assert.equal(out.draft, null);
  assert.equal(JSON.stringify(out.conn).includes('"p"'), false);
});

test("a URL edit does not name an unnamed connection", () => {
  assert.equal(applyUrlEdit(form(), "", "x", parsed()).conn.name, "");
});

test("a URL edit without the TLS files removes them", () => {
  const files = { ssl_root_cert: "C:/certs/ca.pem", ssl_client_cert: "C:/certs/c.pem", ssl_client_key: "C:/certs/c.key" };
  const out = applyUrlEdit(form(files), "", "postgres://u@db.example/shop?sslmode=verify-ca", parsed({ password: null, ssl_mode: "verify-ca" }));
  assert.equal(out.conn.ssl_root_cert, null);
  assert.equal(out.conn.ssl_client_cert, null);
  assert.equal(out.conn.ssl_client_key, null);
  const kept = applyUrlEdit(form(), "", "x", parsed({ password: null, ...files }));
  assert.equal(kept.conn.ssl_root_cert, "C:/certs/ca.pem");
  assert.equal(kept.conn.ssl_client_key, "C:/certs/c.key");
});
