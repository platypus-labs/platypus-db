// Import from DataGrip (dataSources.xml, with the user names of dataSources.local.xml beside
// it): a MySQL source with a port and a user, a PostgreSQL one with an IPv6 host and an escaped
// name, one with a JDBC URL of no known database (left out), and one that is already saved
// (unticked, so only the new ones are imported); a file with no sources and text that is not
// XML show no rows; Cancel saves nothing; the second and third import skip what is saved.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { cancelImport, importDialog, importLabel, importRow, readImportFile, saveImport, savedConnections } from "../import-helpers.mjs";

const SHARED = `<?xml version="1.0" encoding="UTF-8"?>
<project version="4"><component name="DataSourceManagerImpl" format="xml" multifile-model="true">
  <data-source source="LOCAL" name="shop@localhost" uuid="u-1">
    <driver-ref>mysql.8</driver-ref><jdbc-url>jdbc:mysql://localhost:3307/shop?useSSL=false</jdbc-url>
  </data-source>
  <data-source source="LOCAL" name="pg &amp; co" uuid="u-2">
    <jdbc-url>jdbc:postgresql://[::1]/postgres</jdbc-url>
  </data-source>
  <data-source source="LOCAL" name="Weird" uuid="u-3">
    <jdbc-url>jdbc:cobol://mainframe/ledger</jdbc-url>
  </data-source>
  <data-source source="LOCAL" name="already here" uuid="u-4">
    <jdbc-url>jdbc:postgresql://known.example.com:5433/crm</jdbc-url>
  </data-source>
</component></project>`;
const LOCAL = `<project><component name="dataSourceStorageLocal">
  <data-source name="shop@localhost" uuid="u-1"><user-name>shopper</user-name></data-source>
  <data-source name="already here" uuid="u-4"><user-name>crm</user-name></data-source>
</component></project>`;

export function setup({ dir }) {
  mkdirSync(join(dir, "idea"));
  writeFileSync(join(dir, "idea", "dataSources.xml"), SHARED);
  writeFileSync(join(dir, "idea", "dataSources.local.xml"), LOCAL);
  writeFileSync(join(dir, "not-xml.xml"), "this is not a DataGrip file");
  mkdirSync(join(dir, "empty"));
  writeFileSync(join(dir, "empty", "dataSources.xml"), `<project version="4"><component name="DataSourceManagerImpl"></component></project>`);
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000a01"
name = "CRM (mine)"
driver = "postgres"
host = "known.example.com"
port = 5433
database = "crm"
user = "crm"
save_password = false
`,
  );
}

const imports = (n) => `[...${importDialog}.querySelectorAll('.actions button')].some(b => b.textContent.trim() === 'Import ${n}')`;

export default async function (page, { dir }) {
  // A file without sources: the dialog says so and offers nothing to import.
  assert.equal(await readImportFile(page, "datagrip", join(dir, "empty", "dataSources.xml")), null);
  assert.ok((await page.eval(`${importDialog}.textContent`)).includes("No connections found in this file."));
  assert.equal(await page.eval(`[...${importDialog}.querySelectorAll('.actions button')].find(b => b.textContent.trim() === 'Import').disabled`), true);

  // Text that is not a DataGrip file: no sources, or an error; never a row.
  await readImportFile(page, "datagrip", join(dir, "not-xml.xml"));
  assert.equal(await page.eval(`${importDialog}.querySelectorAll('.import-row.connection').length`), 0);

  // The real file: three sources are read, the one of no known database is not.
  assert.equal(await readImportFile(page, "datagrip", join(dir, "idea", "dataSources.xml")), null);
  assert.equal(await page.eval(`${importDialog}.querySelectorAll('.import-row.connection').length`), 3);
  assert.ok((await page.eval(`${importDialog}.textContent`)).includes("Passwords are not imported"));
  assert.equal(await page.eval(`${importRow("Weird")} === undefined`), true);
  // The saved connection with the same host, port, database and user starts unticked.
  assert.equal(await page.eval(`${importRow("already here")}.querySelector('input').checked`), false);
  assert.equal(await page.eval(`${importRow("shop@localhost")}.querySelector('input').checked`), true);
  assert.equal(await importLabel(page), "Import 2");

  // Cancel saves nothing.
  await cancelImport(page);
  assert.deepEqual(Object.keys(savedConnections(dir)), ["CRM (mine)"]);

  // Again, the PostgreSQL one left out too: only the MySQL one is saved, with its user.
  await readImportFile(page, "datagrip", join(dir, "idea", "dataSources.xml"));
  await page.eval(`${importRow("pg & co")}.querySelector('input').click()`);
  await page.waitFor(imports(1));
  await saveImport(page);
  let saved = savedConnections(dir);
  assert.deepEqual(Object.keys(saved).sort(), ["CRM (mine)", "shop@localhost"]);
  const shop = saved["shop@localhost"];
  assert.deepEqual(
    { driver: shop.driver, host: shop.host, port: shop.port, database: shop.database, user: shop.user },
    { driver: "mysql", host: "localhost", port: "3307", database: "shop", user: "shopper" },
  );

  // The third time the MySQL one is a duplicate; the IPv6 one comes with its name decoded and
  // the default port.
  await readImportFile(page, "datagrip", join(dir, "idea", "dataSources.xml"));
  assert.equal(await page.eval(`${importRow("shop@localhost")}.querySelector('input').checked`), false);
  assert.equal(await importLabel(page), "Import 1");
  await saveImport(page);
  saved = savedConnections(dir);
  assert.deepEqual(Object.keys(saved).sort(), ["CRM (mine)", "pg & co", "shop@localhost"]);
  assert.equal(saved["pg & co"].host, "::1");
  assert.equal(saved["pg & co"].port, "5432");
}
