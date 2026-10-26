// Oracle through its native driver: the first connection finds no Instant Client, the popup
// shows Oracle's license and downloads it from Oracle, and the query runs; Settings › Drivers
// lists the client and removes it. Needs the Oracle container
// (`docker compose -f docker/compose.yml --profile oracle up -d --wait`) and the network.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { connect as tcp } from "node:net";

const PORT = 51521;
const NAME = "Oracle Free";
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const popup = `document.querySelector('.driver-popup')`;
const button = (scope, text) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const settingsRow = `document.querySelector('.modal.settings [data-vendor-driver="oracle"]')`;

export function skip() {
  return new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port: PORT, timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolve(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done(`no Oracle on port ${PORT} (docker compose -f docker/compose.yml --profile oracle up -d)`));
    socket.on("timeout", () => done(`no Oracle on port ${PORT}`));
  });
}

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 6

[[connections]]
id = "00000000-0000-4000-8000-000000000071"
name = "${NAME}"
driver = "oracle"
host = "127.0.0.1"
port = ${PORT}
database = "FREEPDB1"
user = "platypus"
ssl_mode = "disable"
save_password = false
`,
  );
}

export default async function (page) {
  await page.waitFor(`window.__platypus !== undefined && ${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.id = 'e2e-conn'`);
  // Expanding the connection connects: no saved password, the app asks for it.
  const rows = await page.eval(`document.querySelectorAll('.tree-row').length`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`, 15_000);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`${popup}?.textContent.includes('Oracle needs Oracle Instant Client')`, 30_000);
  await page.waitFor(`${popup}.textContent.includes('Oracle Free Use Terms and Conditions')`);
  assert.ok((await page.eval(`${popup}.textContent`)).includes("SHA-256"));
  // Nothing to register: no administrator prompt for Oracle.
  assert.equal(await page.eval(`${popup}.querySelector('[data-notice="uac"]')`), null);
  assert.equal(await page.eval(`document.querySelectorAll('.driver-popup').length`), 1);
  await page.eval(`${button(popup, "Download and connect")}.click()`);
  await page.waitFor(`${popup} === null`, 300_000);
  // Connected once the navigator shows the schemas: the editor then reuses the password.
  const tree = `[...document.querySelectorAll('.tree-row')]`;
  await page.waitFor(`${tree}.length > ${rows} && !${tree}.some(r => r.textContent.includes('loading…'))`, 60_000);

  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`, 30_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT 42 AS answer FROM DUAL");
  await page.waitFor(`${visibleTab}.querySelector('.cm-line')?.textContent === 'SELECT 42 AS answer FROM DUAL'`);
  await page.eval(`${visibleTab}.querySelectorAll('.statusbar span').forEach(s => (s.textContent = ''))`);
  await page.key("Enter", "Enter", 13, 2);
  await page.waitFor(`${visibleTab}.querySelector('.statusbar span')?.textContent === '1 row'`, 60_000);

  // Settings › Drivers shows the client and removes it.
  await page.eval(`document.querySelector('button[aria-label="Settings"]').click()`);
  await page.settingsTab("Drivers");
  await page.waitFor(`${settingsRow}?.querySelector('[data-driver-state="installed"]') != null`, 15_000);
  const listed = await page.eval(`${settingsRow}.textContent`);
  assert.ok(listed.includes("Oracle Instant Client 23.26.4 installed"), listed);
  await page.eval(`${button(settingsRow, "Remove")}.click()`);
  await page.waitFor(`${settingsRow}.querySelector('[data-driver-state="missing"]') != null`, 15_000);
}
