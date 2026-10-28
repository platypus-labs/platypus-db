// Unit tests of what an edit of a saved connection means for its open sessions
// (src/connectionEdit.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { hasSessionTimeZone, sessionSettingsChanged, withoutUnusableInit } from "../src/connectionEdit.ts";

const saved = (over = {}) => ({
  id: "c1",
  name: "Local",
  driver: "postgres",
  host: "localhost",
  port: 5432,
  database: "platypus",
  user: "platypus",
  ssl_mode: "disable",
  ssl_root_cert: null,
  save_password: false,
  password_command: "echo platypus",
  read_only: false,
  color: null,
  connection_type: null,
  agent_access: "read_only",
  init: { schema: null, time_zone: null, statements: "" },
  virtual_keys: [],
  ...over,
});

test("what the sessions use is a change", () => {
  for (const over of [
    { database: "postgres" },
    { connection_type: "prod" },
    { host: "db" },
    { port: 5433 },
    { user: "other" },
    { ssl_mode: "require" },
    { password_command: null, save_password: true },
    { init: { schema: "sales", time_zone: null, statements: "" } },
    { ssh: { host: "bastion", port: 22, user: "ops", auth: "agent", key_path: null, secret_id: "s", jump: null } },
  ]) {
    assert.equal(sessionSettingsChanged(saved(), saved(over)), true, JSON.stringify(over));
  }
});

test("what only the app shows or checks is not", () => {
  for (const over of [
    { name: "Renamed" },
    { color: "#c92a2a" },
    { read_only: true },
    { agent_access: "off" },
    { connect_by: "url" },
    { virtual_keys: [{ schema: "public", table: "t", columns: ["id"] }] },
  ]) {
    assert.equal(sessionSettingsChanged(saved(), saved(over)), false, JSON.stringify(over));
  }
  // Nor is the order the backend sends the fields in.
  const { host, ...rest } = saved();
  assert.equal(sessionSettingsChanged(saved(), { ...rest, host }), false);
});

test("a time zone is kept only for drivers whose sessions take one", () => {
  assert.equal(hasSessionTimeZone("postgres"), true);
  assert.equal(hasSessionTimeZone("mysql"), true);
  assert.equal(hasSessionTimeZone("sqlserver"), false);
  assert.equal(hasSessionTimeZone("mongo"), false);
  assert.equal(hasSessionTimeZone("sqlite"), false);
  const zoned = (driver) => saved({ driver, init: { schema: null, time_zone: "Europe/Rome", statements: "SET x" } });
  assert.equal(withoutUnusableInit(zoned("postgres")).init.time_zone, "Europe/Rome");
  const ms = withoutUnusableInit(zoned("sqlserver"));
  assert.equal(ms.init.time_zone, null);
  assert.equal(ms.init.statements, "SET x");
  const plain = saved();
  assert.equal(withoutUnusableInit(plain), plain);
});
