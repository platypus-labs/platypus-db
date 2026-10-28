// Regression guard: Test connection from the form's URL mode reaches every Docker database
// that has one (docker/compose.yml): MySQL and MariaDB (`mariadb://`), ClickHouse (its own
// scheme and `http://`), MongoDB with authSource, libSQL over `http://` with its `authToken`,
// SQL Server through a JDBC URL with encrypt/trustServerCertificate, and the PostgreSQL wire
// variants (TimescaleDB, CockroachDB, YugabyteDB tiles). Each printed URL, read back, gives
// the same connection.
import assert from "node:assert/strict";
import { needsPort, seedSqlite, testConnection } from "../conn-helpers.mjs";
import { LIBSQL_TOKEN } from "../qa-dbs.mjs";
import { cancel, enterUrl, leaveUrl, newUrlForm, url, urlError } from "../qa3-url.mjs";

export const skip = needsPort(53306, "MySQL");

export function setup({ dir }) {
  seedSqlite(dir);
}

const CASES = [
  ["MySQL", "mysql://platypus:platypus@localhost:53306/platypus"],
  ["MariaDB", "mariadb://platypus:platypus@127.0.0.1:53307/platypus"],
  ["ClickHouse", "clickhouse://platypus:platypus@localhost:58123/platypus?sslmode=disable"],
  ["ClickHouse", "http://platypus:platypus@localhost:58123/platypus"],
  ["MongoDB", "mongodb://platypus:platypus@localhost:57017/admin?authSource=admin"],
  ["Turso / libSQL", `http://localhost:58080?authToken=${LIBSQL_TOKEN}`],
  ["SQL Server", "jdbc:sqlserver://localhost:51433;databaseName=master;user=sa;password=Platypus-2026;encrypt=true;trustServerCertificate=true"],
  ["TimescaleDB", "postgres://platypus:platypus@localhost:55433/platypus?sslmode=disable"],
  ["CockroachDB", "postgresql://root@localhost:56257/platypus?sslmode=disable"],
  ["YugabyteDB", "postgres://yugabyte@localhost:55434/yugabyte?sslmode=disable"],
];

export default async function (page) {
  const failures = [];
  for (const [tile, address] of CASES) {
    await newUrlForm(page, tile);
    await enterUrl(page, address);
    const error = await page.eval(urlError);
    if (error) {
      failures.push(`${tile} ${address}: ${error}`);
      await cancel(page);
      continue;
    }
    const result = await testConnection(page, 40_000);
    if (!result.startsWith("Connected")) failures.push(`${tile} ${address}: ${result}`);
    // The printed URL read again changes nothing.
    const printed = await leaveUrl(page);
    await page.eval(`${url}.focus()`);
    await enterUrl(page, printed);
    await leaveUrl(page);
    if ((await page.eval(`${url}.value`)) !== printed) failures.push(`${tile}: ${printed} reads back as ${await page.eval(`${url}.value`)}`);
    await cancel(page);
  }
  assert.deepEqual(failures, []);
}
