// A database reached through the generic ODBC driver: SQL Server through Windows' own
// `SQL Server` driver. The query runs, and Settings › Drivers lists the driver among the
// system's. Needs the SQL Server container
// (`docker compose -f docker/compose.yml --profile sqlserver up -d --wait`).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { connect as tcp } from "node:net";

const PORT = 51433;
const NAME = "SQL Server via ODBC";
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;

export function skip() {
  return new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port: PORT, timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolve(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done(`no SQL Server on port ${PORT} (docker compose -f docker/compose.yml --profile sqlserver up -d)`));
    socket.on("timeout", () => done(`no SQL Server on port ${PORT}`));
  });
}

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 6

[[connections]]
id = "00000000-0000-4000-8000-000000000081"
name = "${NAME}"
driver = "odbc"
host = ""
database = ""
user = "sa"
save_password = false

[connections.options]
driver = "SQL Server"
connection_string = "Server=127.0.0.1,${PORT};Database=platypus"
`,
  );
}

export default async function (page) {
  await page.waitFor(`window.__platypus !== undefined && ${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`, 15_000);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("Platypus-2026");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);

  await page.waitFor(`${visibleTab} !== undefined`, 30_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT 42 AS answer");
  await page.waitFor(`${visibleTab}.querySelector('.cm-line')?.textContent === 'SELECT 42 AS answer'`);
  await page.eval(`${visibleTab}.querySelectorAll('.statusbar span').forEach(s => (s.textContent = ''))`);
  await page.key("Enter", "Enter", 13, 2);
  await page.waitFor(`${visibleTab}.querySelector('.statusbar span')?.textContent === '1 row'`, 30_000);

  // Settings › Drivers lists the system's ODBC drivers.
  await page.eval(`document.querySelector('button[aria-label="Settings"]').click()`);
  await page.waitFor(`document.querySelector('.modal.settings .odbc-system') !== null`, 15_000);
  const listed = await page.eval(`document.querySelector('.modal.settings .odbc-system').textContent`);
  assert.ok(listed.includes("SQL Server"), listed);
}
