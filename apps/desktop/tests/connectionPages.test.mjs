// Unit tests of which dialog pages show the "has settings" dot (src/components/connectionPages.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { initHasSettings, networkHasSettings } from "../src/components/connectionPages.ts";

const base = { ssl_mode: "prefer", ssl_root_cert: null, options: {}, init: { schema: null, time_zone: null, statements: "" } };

test("a plain connection has no dot on either page", () => {
  assert.equal(networkHasSettings(base), false);
  assert.equal(initHasSettings(base), false);
});

test("each network setting shows the dot", () => {
  assert.equal(networkHasSettings({ ...base, ssl_mode: "require" }), true);
  assert.equal(networkHasSettings({ ...base, ssl_mode: "disable" }), true);
  assert.equal(networkHasSettings({ ...base, ssl_root_cert: "C:\\ca.pem" }), true);
  assert.equal(networkHasSettings({ ...base, ssl_client_key: "k.pem" }), true);
  assert.equal(networkHasSettings({ ...base, network_profile: "p" }), true);
  assert.equal(networkHasSettings({ ...base, ssh: { host: "h" } }), true);
  for (const k of ["proxy", "forward", "before_connect", "after_disconnect"]) {
    assert.equal(networkHasSettings({ ...base, options: { [k]: "x" } }), true, k);
  }
});

test("blank values do not count", () => {
  assert.equal(networkHasSettings({ ...base, options: { proxy: "  " } }), false);
  assert.equal(initHasSettings({ ...base, init: { schema: " ", time_zone: null, statements: "\n" } }), false);
});

test("each init setting shows the dot", () => {
  assert.equal(initHasSettings({ ...base, init: { ...base.init, schema: "app" } }), true);
  assert.equal(initHasSettings({ ...base, init: { ...base.init, time_zone: "UTC" } }), true);
  assert.equal(initHasSettings({ ...base, init: { ...base.init, statements: "SELECT 1" } }), true);
  assert.equal(initHasSettings({ ...base, options: { env_file: ".env" } }), true);
});
