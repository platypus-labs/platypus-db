// QA5-CONN-4 (medium). A pg_service.conf service without `dbname` is imported with the
// SERVICE NAME as its database: `[prod] host=db.example.com user=app` becomes
// app@db.example.com:5432/prod. libpq (and psql "service=prod") connect to the database named
// after the user (here "app"); the service's name is never a database, so the imported
// connection fails with `database "prod" does not exist`.
// Steps: Import connections > pg_service; choose a file with
//   `[prod]\nhost=db.example.com\nuser=app`.
// Expected: the row shows database "app" (the user, as libpq), or "postgres".
// Actual: the row reads app@db.example.com:5432/prod.
// Code: crates/platypus-store/src/import.rs `pg_service`: `name.clone_into(&mut c.database)`.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { importDialog, readImportFile } from "../import-helpers.mjs";
import { seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page, ctx) {
  const file = join(ctx.dir, "pg_service.conf");
  writeFileSync(file, "[prod]\nhost=db.example.com\nuser=app\n");
  assert.equal(await readImportFile(page, "pg_service", file), null);
  const text = await page.eval(`${importDialog}.querySelector('.import-row.connection').innerText.replace(/\\s+/g, ' ')`);
  assert.doesNotMatch(text, /\/prod\b/, `the service name was taken for the database: ${text}`);
}
