// Commands around a connection (Connection › Network › Command before connecting / after
// disconnecting), run by the system's shell: a failed command before stops the connection
// and its error says why, quoting what it wrote on stderr; one that ends well lets the
// connection be tried, and the command after runs when it closes, here as it fails (closed
// port). Saved with the connection, they mark the Network tab, and blank ones are none.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, editConnection, fill, saveForm, testConnection } from "../conn-helpers.mjs";

const NAME = "E2E commands";
const before = `document.querySelector('.connection-dialog [data-field=before_connect]')`;
const after = `document.querySelector('.connection-dialog [data-field=after_disconnect]')`;
const networkTab = `[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network'))`;

export function setup({ dir }) {
  // A closed port: the commands run, then the connection fails.
  const toml = `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000f0301"
name = "${NAME}"
driver = "postgres"
host = "127.0.0.1"
port = 1
database = "x"
user = "x"
ssl_mode = "disable"
`;
  writeFileSync(join(dir, "connections.toml"), toml);
}

async function waitForFile(page, file) {
  for (let i = 0; i < 100 && !existsSync(file); i++) await page.sleep(100);
  assert.ok(existsSync(file), `${file} written`);
}

export default async function (page, { dir }) {
  await editConnection(page, NAME);
  assert.ok(!(await page.eval(`${networkTab}.textContent`)).includes("●"), "nothing set on the Network tab");
  await page.eval(`${networkTab}.click()`);
  await page.waitFor(`${before} !== null`);

  // A failed command before: the connection is not tried, the error quotes stderr.
  await page.eval(fill(before, "echo VPN refused 1>&2 & exit /b 3"));
  const failed = await testConnection(page);
  assert.match(failed, /command before connecting/, failed);
  assert.match(failed, /VPN refused/, failed);
  assert.match(failed, /3/, failed);

  // One that ends well: the connection is tried (and refused), the command after runs.
  const up = join(dir, "up.txt");
  const down = join(dir, "down.txt");
  await page.eval(fill(before, `echo up> "${up}"`));
  await page.eval(fill(after, `echo down> "${down}"`));
  const tried = await testConnection(page);
  assert.doesNotMatch(tried, /command before connecting/, tried);
  assert.ok(existsSync(up), "the command before ran first");
  await waitForFile(page, down);
  assert.equal(readFileSync(down, "utf8").trim(), "down");

  // Saved with the connection: the Network tab is marked, the commands come back.
  await saveForm(page);
  const saved = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.ok(saved.includes("before_connect") && saved.includes("after_disconnect"), saved);
  await editConnection(page, NAME);
  assert.ok((await page.eval(`${networkTab}.textContent`)).includes("●"), "the Network tab is marked");
  await page.eval(`${networkTab}.click()`);
  await page.waitFor(`${before}?.value === ${JSON.stringify(`echo up> "${up}"`)}`);

  // Blank commands are none: the test no longer runs them.
  await page.eval(fill(before, "   "));
  await page.eval(fill(after, ""));
  const plain = await testConnection(page);
  assert.doesNotMatch(plain, /command before connecting/, plain);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null`);
}
