// Import from pgAdmin (servers.json of Tools › Export Servers): servers of the top level come in
// the order of their ids after the folders, the group becomes a folder (the default group "Servers" does not), the SSL mode,
// the maintenance database and an SSH tunnel come along, a server without a name still gets
// one; a file without "Servers" and text that is not JSON show an error; an export with no
// servers says so; Cancel saves nothing.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { cancelImport, importDialog, readImportFile, saveImport, savedConnections } from "../import-helpers.mjs";

const SERVERS = {
  Servers: {
    2: { Name: "Reports", Group: "Team", Host: "reports.example.com", Port: 5433, MaintenanceDB: "analytics", Username: "ro", SSLMode: "require" },
    1: { Name: "Main", Group: "Servers", Host: "db.example.com", Port: 5432, MaintenanceDB: "postgres", Username: "admin", UseSSHTunnel: 1, TunnelHost: "bastion", TunnelPort: "2222", TunnelUsername: "ops", TunnelAuthentication: 0 },
    3: { Host: "unnamed.example.com", Username: "x" },
  },
};

// No connection is saved: the sidebar's import button is what the scenario waits for.
export const ready = `document.querySelector('.sidebar-header button[title^="Import connections"]') !== null`;

export function setup({ dir }) {
  writeFileSync(join(dir, "servers.json"), JSON.stringify(SERVERS));
  writeFileSync(join(dir, "no-servers.json"), JSON.stringify({ Groups: {} }));
  writeFileSync(join(dir, "broken.json"), "{ not json");
  writeFileSync(join(dir, "none.json"), JSON.stringify({ Servers: {} }));
}

export default async function (page, { dir }) {
  // Not a pgAdmin export: the error says what is missing; broken JSON has its own.
  assert.match((await readImportFile(page, "pgadmin", join(dir, "no-servers.json"))) ?? "", /Servers/);
  assert.ok(await readImportFile(page, "pgadmin", join(dir, "broken.json")));
  assert.equal(await page.eval(`${importDialog}.querySelectorAll('.import-row.connection').length`), 0);
  // An export with no servers is a valid file with nothing in it.
  assert.equal(await readImportFile(page, "pgadmin", join(dir, "none.json")), null);
  assert.ok((await page.eval(`${importDialog}.textContent`)).includes("No connections found in this file."));

  // The export: three servers; the group is a folder row, the default group is not.
  assert.equal(await readImportFile(page, "pgadmin", join(dir, "servers.json")), null);
  const names = await page.eval(`[...${importDialog}.querySelectorAll('.import-row.connection .import-name')].map(n => n.textContent)`);
  assert.equal(names.length, 3, JSON.stringify(names));
  // The folder's server comes first, then those of the top level in the order of their ids.
  assert.deepEqual(names.slice(0, 2), ["Reports", "Main"]);
  assert.deepEqual(await page.eval(`[...${importDialog}.querySelectorAll('.import-row.folder')].map(r => r.textContent)`), ["Team"]);

  // Cancel saves nothing.
  await cancelImport(page);
  assert.deepEqual(Object.keys(savedConnections(dir)), []);

  await readImportFile(page, "pgadmin", join(dir, "servers.json"));
  await saveImport(page);
  const saved = savedConnections(dir);
  assert.equal(Object.keys(saved).length, 3, JSON.stringify(Object.keys(saved)));
  assert.equal(saved.Main.host, "db.example.com");
  assert.equal(saved.Main.user, "admin");
  assert.equal(saved.Main.database, "postgres");
  assert.equal(saved.Reports.port, "5433");
  assert.equal(saved.Reports.database, "analytics");
  assert.equal(saved.Reports.ssl_mode, "require");
  // The unnamed server has a name made up, and keeps its host.
  const unnamed = Object.values(saved).find((c) => c.host === "unnamed.example.com");
  assert.ok(unnamed?.name, "a name was made up");
  // The SSH tunnel of Main.
  const text = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.match(text, /host = "bastion"/);
  assert.match(text, /port = 2222/);
  assert.match(text, /user = "ops"/);
}
