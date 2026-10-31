// Import from libpq's pg_service.conf: each [service] is a connection (the service name is the
// default database), comments and blank lines are skipped, hostaddr stands for host, sslmode
// and port come along, unknown keys are ignored; a file with no service says so; a service that
// is already saved starts unticked; only the ticked ones are saved.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { importDialog, importLabel, importRow, readImportFile, saveImport, savedConnections } from "../import-helpers.mjs";

const CONF = `# services of the team
[reports]
host=r.example.com
port=5440
dbname=rep
user=ro
sslmode=require
connect_timeout=10

; the local one
[local]
hostaddr=127.0.0.1

[warehouse]
host=w.example.com
dbname=wh
`;

export function setup({ dir }) {
  writeFileSync(join(dir, "pg_service.conf"), CONF);
  writeFileSync(join(dir, "empty.conf"), "# nothing here\n\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000b01"
name = "wh already"
driver = "postgres"
host = "w.example.com"
port = 5432
database = "wh"
user = ""
save_password = false
`,
  );
}

export default async function (page, { dir }) {
  // No service in the file: nothing to import.
  assert.equal(await readImportFile(page, "pg_service", join(dir, "empty.conf")), null);
  assert.ok((await page.eval(`${importDialog}.textContent`)).includes("No connections found in this file."));

  assert.equal(await readImportFile(page, "pg_service", join(dir, "pg_service.conf")), null);
  const names = await page.eval(`[...${importDialog}.querySelectorAll('.import-row.connection .import-name')].map(n => n.textContent)`);
  assert.deepEqual(names, ["reports", "local", "warehouse"]);
  // The service the app has already (same host, port, database, user) starts unticked.
  assert.equal(await page.eval(`${importRow("warehouse")}.querySelector('input').checked`), false);
  assert.equal(await importLabel(page), "Import 2");
  // Leave `local` out as well.
  await page.eval(`${importRow("local")}.querySelector('input').click()`);
  await page.waitFor(`[...${importDialog}.querySelectorAll('.actions button')].some(b => b.textContent.trim() === 'Import 1')`);
  await saveImport(page);

  const saved = savedConnections(dir);
  assert.deepEqual(Object.keys(saved).sort(), ["reports", "wh already"]);
  const r = saved.reports;
  assert.deepEqual(
    { host: r.host, port: r.port, database: r.database, user: r.user, ssl_mode: r.ssl_mode },
    { host: "r.example.com", port: "5440", database: "rep", user: "ro", ssl_mode: "require" },
  );

  // The file again: `reports` is now saved, the local one (hostaddr) is the one left to take,
  // and its database is the service name.
  await readImportFile(page, "pg_service", join(dir, "pg_service.conf"));
  assert.equal(await importLabel(page), "Import 1");
  assert.equal(await page.eval(`${importRow("local")}.querySelector('input').checked`), true);
  await saveImport(page);
  const after = savedConnections(dir);
  assert.equal(after.local.host, "127.0.0.1");
  assert.equal(after.local.database, "local");
}
