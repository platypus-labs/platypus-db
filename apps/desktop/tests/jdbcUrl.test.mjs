// Unit tests of the JDBC URL a driver's template makes from the connection's fields
// (src/jdbcUrl.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { fillUrl, jdbcOn, refillUrl } from "../src/jdbcUrl.ts";

const parts = (template, over = {}) => ({ template, host: "db.example.com", port: 9092, database: "sales", ...over });
const H2 = "jdbc:h2:tcp://{host}:{port}/{database}";
const HANA = "jdbc:sap://{host}:{port}/?databaseName={database}";
const EXASOL = "jdbc:exa:{host}:{port};schema={database}";

test("the template takes the host, the port and the database", () => {
  assert.equal(fillUrl(parts(H2)), "jdbc:h2:tcp://db.example.com:9092/sales");
  assert.equal(fillUrl(parts(HANA, { port: 30015, database: "HXE" })), "jdbc:sap://db.example.com:30015/?databaseName=HXE");
});

test("no host, no URL", () => {
  assert.equal(fillUrl(parts(H2, { host: "  " })), "");
});

test("a part left empty goes with its key", () => {
  assert.equal(fillUrl(parts(HANA, { port: 30015, database: "" })), "jdbc:sap://db.example.com:30015/");
  assert.equal(fillUrl(parts(EXASOL, { port: 8563, database: "" })), "jdbc:exa:db.example.com:8563");
  assert.equal(fillUrl(parts(EXASOL, { port: 8563, database: "S" })), "jdbc:exa:db.example.com:8563;schema=S");
});

test("no port leaves the colon out; an IPv6 host takes brackets", () => {
  assert.equal(fillUrl(parts(H2, { port: null })), "jdbc:h2:tcp://db.example.com/sales");
  assert.equal(fillUrl(parts(H2, { host: "::1" })), "jdbc:h2:tcp://[::1]:9092/sales");
  assert.equal(fillUrl(parts(H2, { host: "[::1]" })), "jdbc:h2:tcp://[::1]:9092/sales");
});

test("a URL the fields made follows them", () => {
  const before = parts(H2, { host: "a" });
  const after = parts(H2, { host: "b" });
  assert.equal(refillUrl(before, after, "jdbc:h2:tcp://a:9092/sales"), "jdbc:h2:tcp://b:9092/sales");
  assert.equal(refillUrl(parts(H2, { host: "" }), before, ""), "jdbc:h2:tcp://a:9092/sales");
  assert.equal(refillUrl(before, after, "  "), "jdbc:h2:tcp://b:9092/sales");
});

test("a URL written by hand is kept", () => {
  const before = parts(H2, { host: "a" });
  const after = parts(H2, { host: "b" });
  assert.equal(refillUrl(before, after, "jdbc:h2:mem:test"), "jdbc:h2:mem:test");
  // Also when another driver's template takes over.
  assert.equal(refillUrl(before, parts(HANA, { host: "a" }), "jdbc:h2:mem:test"), "jdbc:h2:mem:test");
  assert.equal(refillUrl(before, parts(HANA, { host: "a", port: 30015 }), "jdbc:h2:tcp://a:9092/sales"), "jdbc:sap://a:30015/?databaseName=sales");
});

test("a driver, a version or files send a connection through JDBC", () => {
  assert.equal(jdbcOn(undefined), false);
  assert.equal(jdbcOn({ "jdbc.java": "C:\\jdk" }), false);
  assert.equal(jdbcOn({ "jdbc.package": "h2" }), true);
  assert.equal(jdbcOn({ "jdbc.driver": "12.1.5.0" }), true);
  assert.equal(jdbcOn({ "jdbc.jars": "a.jar", "jdbc.driver": " " }), true);
  assert.equal(jdbcOn({ "jdbc.jars": " " }), false);
});
