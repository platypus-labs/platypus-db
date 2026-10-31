// QA4-CONN-6 (low). A DBeaver connection to ClickHouse (provider "clickhouse", driver
// "com_clickhouse", `jdbc:clickhouse://host:8123/db`) is listed as "Not imported: unsupported
// database (clickhouse · com_clickhouse)" although PlatypusDB has a ClickHouse driver and its
// JDBC parser reads `jdbc:clickhouse:`. The same for DuckDB (`jdbc:duckdb:file`), Oracle
// (`jdbc:oracle:thin:@//h:1521/svc`) and libSQL/Snowflake: DBeaver users with these lose them.
// Steps: Import connections > DBeaver; choose a data-sources.json with one ClickHouse
// connection (host ch.example.com, port 8123, database logs, url jdbc:clickhouse://…).
// Expected: the connection is listed to import (ch.example.com:8123/logs).
// Actual: "Not imported: unsupported database".
// Code: crates/platypus-store/src/import.rs `read_jdbc` (only Postgres, MySql, Sqlite,
// SqlServer pass the filter) and `dbeaver_driver` (no clickhouse/duckdb/oracle provider ids).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { importDialog, readImportFile } from "../import-helpers.mjs";
import { seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page, ctx) {
  const file = join(ctx.dir, "data-sources.json");
  writeFileSync(
    file,
    JSON.stringify({
      connections: {
        a: {
          provider: "clickhouse",
          driver: "com_clickhouse",
          name: "ch real",
          configuration: { host: "ch.example.com", port: "8123", database: "logs", url: "jdbc:clickhouse://ch.example.com:8123/logs", "auth-model": "native" },
        },
      },
    }),
  );
  const error = await readImportFile(page, "dbeaver", file);
  assert.equal(error, null);
  const text = await page.eval(`${importDialog}.innerText`);
  assert.doesNotMatch(text, /Not imported/, text);
  assert.match(text, /ch\.example\.com:8123/, text);
}
