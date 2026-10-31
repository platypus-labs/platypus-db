// QA5-CONN-3 (low). Import from a pg_service file or a pgAdmin servers.json saved with a
// UTF-8 byte-order mark (Windows Notepad, PowerShell 5 redirection) goes wrong: the first
// `[service]` of a pg_service.conf is silently left out (its line starts with U+FEFF, not `[`),
// and a pgAdmin file fails with "expected value at line 1 column 1". DBeaver's reader strips
// the mark already.
// Steps: Import connections > pg_service, choose a BOM file with services [first] and
//   [second]; then pgAdmin, choose a BOM servers.json with one server.
// Expected: both services are listed; the pgAdmin server is listed.
// Actual: only [second]; the pgAdmin file is refused.
// Code: crates/platypus-store/src/import.rs `import` (`read_text` = from_utf8_lossy keeps U+FEFF;
//   only the DBeaver branch strips it).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { importDialog, readImportFile } from "../import-helpers.mjs";
import { seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

const rowsText = (page) => page.eval(`[...${importDialog}.querySelectorAll('.import-row.connection')].map(r => r.innerText.replace(/\\s+/g, ' ')).join('|')`);

export default async function (page, ctx) {
  const failures = [];
  const service = join(ctx.dir, "pg_service.conf");
  writeFileSync(service, "\uFEFF[first]\nhost=h1\ndbname=d1\n\n[second]\nhost=h2\ndbname=d2\n");
  const err1 = await readImportFile(page, "pg_service", service);
  const text1 = await rowsText(page);
  if (err1 || !/first/.test(text1) || !/second/.test(text1)) failures.push(`pg_service: error ${err1}, rows "${text1}"`);

  const admin = join(ctx.dir, "servers.json");
  writeFileSync(admin, `\uFEFF${JSON.stringify({ Servers: { 1: { Name: "adm", Host: "h", Port: 5432, MaintenanceDB: "postgres", Username: "u" } } })}`);
  const err2 = await readImportFile(page, "pgadmin", admin);
  const text2 = await rowsText(page);
  if (err2 || !/adm/.test(text2)) failures.push(`pgAdmin: error ${err2}, rows "${text2}"`);
  assert.deepEqual(failures, [], failures.join(" | "));
}
