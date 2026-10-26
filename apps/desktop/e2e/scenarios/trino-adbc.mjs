// A database reached through an ADBC driver: the first connection to Trino finds no driver,
// the popup downloads it from the driver registry, and the query runs; Settings › Drivers
// lists the driver and removes it. Needs the Trino container
// (`docker compose -f docker/compose.yml --profile trino up -d --wait`) and the network.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { connect as tcp } from "node:net";

const PORT = 58085;
const NAME = "Trino memory";
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const popup = `document.querySelector('.driver-popup')`;
const button = (scope, text) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

export function skip() {
  return new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port: PORT, timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolve(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done(`no Trino on port ${PORT} (docker compose -f docker/compose.yml --profile trino up -d)`));
    socket.on("timeout", () => done(`no Trino on port ${PORT}`));
  });
}

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 6

[[connections]]
id = "00000000-0000-4000-8000-000000000061"
name = "${NAME}"
driver = "trino"
host = "127.0.0.1"
port = ${PORT}
database = "memory"
user = "platypus"
ssl_mode = "disable"
save_password = true

[connections.options]
schema = "default"
`,
  );
}

export default async function (page) {
  await page.waitFor(`window.__platypus !== undefined && ${treeRow(NAME)} !== undefined`);

  // Opening the tab connects the navigator, which finds no driver: the popup offers it. The
  // popup is a modal, so the run is started only once it is open (a key pressed before it
  // appears may or may not reach the editor).
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT 42 AS answer");
  await page.waitFor(`${visibleTab}.querySelector('.cm-line')?.textContent === 'SELECT 42 AS answer'`);
  await page.waitFor(`${popup}?.textContent.includes('Trino needs a driver')`, 15_000);
  // The editor's session asks for the same driver while the popup is open: it joins it.
  await page.eval(`${visibleTab}.querySelectorAll('.statusbar span').forEach(s => (s.textContent = ''))`);
  await page.eval(`${button(visibleTab, "Run")}.click()`);
  // The registry answers with the package.
  await page.waitFor(`${popup}.textContent.includes('Apache-2.0')`, 30_000);
  assert.equal(await page.eval(`document.querySelectorAll('.driver-popup').length`), 1);
  await page.eval(`${button(popup, "Download and connect")}.click()`);
  await page.waitFor(`${popup} === null`, 120_000);

  // The query runs through the downloaded driver.
  await page.waitFor(`${visibleTab}.querySelector('.statusbar span')?.textContent === '1 row'`, 30_000);

  // Settings › Drivers lists it and removes it.
  await page.eval(`document.querySelector('button[aria-label="Settings"]').click()`);
  await page.settingsTab("Drivers");
  await page.waitFor(`document.querySelector('.modal.settings [data-driver="trino"]') !== null`);
  const listed = await page.eval(`document.querySelector('.modal.settings [data-driver="trino"]').textContent`);
  assert.ok(listed.includes("Trino"), listed);
  await page.eval(`${button("document.querySelector('.modal.settings [data-driver=\"trino\"]')", "Remove")}.click()`);
  await page.waitFor(`document.querySelector('.modal.settings [data-driver="trino"]') === null`);
}
