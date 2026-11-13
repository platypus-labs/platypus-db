// Bug NAV (low): on CockroachDB (PostgreSQL driver) the system schemas `crdb_internal` and
// `pg_extension` are listed among the user schemas, before `demo` and `public`, not muted and
// not hidden by the Simple view, while `information_schema` and `pg_catalog` are.
//
// Cause: `schemas` in crates/platypus-driver-pg/src/meta.rs flags only `pg_catalog` and
// `information_schema` as system.
//
// Steps: connect to CockroachDB (docker profile pgwire, port 56257, user root).
// Expected: crdb_internal and pg_extension muted with the catalog schemas. Actual: shown as
// user schemas.
import assert from "node:assert/strict";
import { connect as tcp } from "node:net";
import { row, saveConnections } from "../navigator-helpers.mjs";

export function skip() {
  return new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port: 56257, timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolve(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done("no CockroachDB on port 56257 (docker compose -f docker/compose.yml --profile pgwire up -d)"));
    socket.on("timeout", () => done("no CockroachDB on port 56257"));
  });
}

export function setup({ dir }) {
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a0071"
name = "QA Cockroach"
driver = "postgres"
host = "localhost"
port = 56257
database = "platypus"
user = "root"
ssl_mode = "disable"
save_password = false
`);
}

export default async function (page) {
  await page.eval(`${row("QA Cockroach")}.querySelector('.twisty').click()`);
  // No password: the prompt, if any, is answered empty.
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null || ${row("crdb_internal")} !== undefined`, 20_000);
  if (await page.eval(`document.querySelector('.modal input[type=password]') !== null`)) {
    await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  }
  await page.waitFor(`${row("crdb_internal")} !== undefined && ${row("pg_catalog")} !== undefined`, 20_000);
  assert.ok(await page.eval(`${row("pg_catalog")}.classList.contains('muted')`), "pg_catalog is muted");
  for (const schema of ["crdb_internal", "pg_extension"]) {
    assert.ok(await page.eval(`${row(schema)}.classList.contains('muted')`), `${schema} is a system schema`);
  }
}
