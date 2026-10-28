// Runs the conformance suite on the older versions of the databases, those of the `versions`
// profile of docker/compose.yml. Each run points one database's variable at an older server,
// which makes it required; the other databases stay as they are (skipped when not started).
//
//   docker compose -f docker/compose.yml --profile versions up -d --wait
//   node scripts/conformance-versions.mjs            # every version
//   node scripts/conformance-versions.mjs mysql-5.7  # those whose name contains the filter
import { spawnSync } from "node:child_process";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));

/** Each version: the variable the suite reads and the server's URL. */
const VERSIONS = [
  ["postgres-12", "PLATYPUS_TEST_PG_URL", "postgres://platypus:platypus@localhost:55412/platypus?sslmode=disable"],
  ["postgres-14", "PLATYPUS_TEST_PG_URL", "postgres://platypus:platypus@localhost:55414/platypus?sslmode=disable"],
  ["postgres-16", "PLATYPUS_TEST_PG_URL", "postgres://platypus:platypus@localhost:55416/platypus?sslmode=disable"],
  ["mysql-5.7", "PLATYPUS_TEST_MYSQL_URL", "mysql://platypus:platypus@localhost:53357/platypus"],
  ["mysql-8.0", "PLATYPUS_TEST_MYSQL_URL", "mysql://platypus:platypus@localhost:53380/platypus"],
  ["mariadb-10.6", "PLATYPUS_TEST_MARIADB_URL", "mysql://platypus:platypus@localhost:53406/platypus"],
  ["sqlserver-2017", "PLATYPUS_TEST_MSSQL_URL", "sqlserver://sa:Platypus-2026@localhost:51417/platypus?sslmode=require"],
  ["mongo-5", "PLATYPUS_TEST_MONGO", "mongodb://platypus:platypus@localhost:57005/platypus?authSource=admin"],
];

const filter = process.argv[2] ?? "";
const failed = [];
for (const [name, variable, url] of VERSIONS.filter(([name]) => name.includes(filter))) {
  console.log(`\n== ${name}`);
  const run = spawnSync("cargo", ["test", "-q", "-p", "platypus-conformance"], {
    cwd: root,
    stdio: "inherit",
    env: { ...process.env, [variable]: url },
    shell: process.platform === "win32",
  });
  if (run.status !== 0) failed.push(name);
}
if (failed.length > 0) {
  console.error(`\nFailed on: ${failed.join(", ")}`);
  process.exit(1);
}
console.log("\nEvery version passed.");
