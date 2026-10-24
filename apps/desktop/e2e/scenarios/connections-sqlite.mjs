// Connectivity: connections imported from DBeaver keep their type; a production connection
// shows its badge and opens in manual commit; the connection dialog has the SSH tunnel.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(text)}))`;

export function setup({ dir }) {
  const db = join(dir, "prod.db");
  const sqlite = new DatabaseSync(db);
  sqlite.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);");
  sqlite.close();
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
  // DBeaver's file where it keeps it on Windows, under the APPDATA the app is given.
  const dbeaver = join(dir, "appdata", "DBeaverData", "workspace6", "General", ".dbeaver");
  mkdirSync(dbeaver, { recursive: true });
  writeFileSync(
    join(dbeaver, "data-sources.json"),
    JSON.stringify({
      connections: {
        "sqlite-1": {
          provider: "generic",
          driver: "sqlite_jdbc",
          name: "Prod copy",
          configuration: { url: `jdbc:sqlite:${db}`, type: "prod" },
        },
        "postgres-jdbc-1": {
          provider: "postgresql",
          driver: "postgres-jdbc",
          name: "Remote PG",
          configuration: {
            host: "db.example.com",
            port: "5432",
            database: "app",
            user: "me",
            type: "dev",
            handlers: { ssh_tunnel: { enabled: true, properties: { host: "bastion", port: "22", user: "ops", authType: "PASSWORD" } } },
          },
        },
      },
    }),
  );
}

export const env = ({ dir }) => ({ APPDATA: join(dir, "appdata") });

export default async function (page, { dir, shots }) {
  await page.waitFor(`document.querySelector('.empty-import') !== null`);
  await page.eval(`document.querySelector('.empty-import').click()`);
  await page.waitFor(`document.querySelector('.import-found') !== null`);
  await page.eval(`${button("document.querySelector('.import-found')", "DBeaver")}.click()`);
  await page.waitFor(`document.querySelectorAll('.import-list label').length === 2`);
  await page.eval(`${button("document.querySelector('.modal')", "Import")}.click()`);
  await page.waitFor(`${treeRow("Prod copy")} !== undefined && ${treeRow("Remote PG")} !== undefined`);

  const saved = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.match(saved, /connection_type = "prod"/);
  assert.match(saved, /\[connections\.ssh\]\s+host = "bastion"/);
  assert.equal(await page.eval(`${treeRow("Prod copy")}.querySelector('.type-badge')?.textContent`), "PROD");

  // Production starts in manual commit.
  await page.eval(`${treeRow("Prod copy")}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT * FROM t");
  await page.key("Enter", "Enter", 13, 2);
  await page.waitFor(`${visibleTab}.querySelector('.statusbar span')?.textContent === '1 row'`, 15_000);
  await page.waitFor(`${button(visibleTab, "Manual commit")} !== undefined`);

  // The dialog of the imported connection shows its SSH server.
  await page.eval(`[...${treeRow("Remote PG")}.querySelectorAll('button')].find(b => b.title === 'Edit').click()`);
  await page.waitFor(`document.querySelector('.ssh-section') !== null`);
  assert.equal(await page.eval(`document.querySelector('.ssh-section input').value`), "bastion");
  if (shots) await page.screenshot(join(shots, "connection-ssh.png"));
}
