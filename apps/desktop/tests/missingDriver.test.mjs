// Unit tests of the check that offers a driver download when a database is picked (src/drivers/missingDriver.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { driverToDownload } from "../src/drivers/missingDriver.ts";

const lookups = (over = {}) => ({
  downloaded: ["trino", "snowflake"],
  vendor: ["oracle", "duckdb"],
  listDrivers: async () => [],
  vendorStatus: async () => ({ package: { name: "x" }, ready: false }),
  ...over,
});

test("a database with nothing to download asks nothing and reads nothing", async () => {
  const fail = async () => assert.fail("looked up");
  assert.equal(await driverToDownload("postgres", lookups({ listDrivers: fail, vendorStatus: fail })), false);
  assert.equal(await driverToDownload("odbc", lookups({ listDrivers: fail, vendorStatus: fail })), false);
});

test("an ADBC driver is offered until it is installed", async () => {
  assert.equal(await driverToDownload("trino", lookups()), true);
  const listDrivers = async () => [{ name: "trino" }];
  assert.equal(await driverToDownload("trino", lookups({ listDrivers })), false);
  assert.equal(await driverToDownload("snowflake", lookups({ listDrivers })), true);
});

test("a vendor library is offered when the app has a package and it is not ready", async () => {
  assert.equal(await driverToDownload("oracle", lookups()), true);
  assert.equal(await driverToDownload("oracle", lookups({ vendorStatus: async () => ({ package: { name: "x" }, ready: true }) })), false);
  // No package for this system: nothing to download.
  assert.equal(await driverToDownload("duckdb", lookups({ vendorStatus: async () => ({ package: null, ready: false }) })), false);
});

test("a failed lookup offers nothing and does not throw", async () => {
  const boom = async () => {
    throw new Error("offline");
  };
  assert.equal(await driverToDownload("trino", lookups({ listDrivers: boom })), false);
  assert.equal(await driverToDownload("oracle", lookups({ vendorStatus: boom })), false);
});
