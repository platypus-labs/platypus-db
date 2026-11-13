// QA3 MCP and the master password: the user turns the master password on in Settings and
// saves a PostgreSQL connection's password from the dialog (into the encrypted file). An
// agent then calls the tools on that connection while the app is open and unlocked: the
// server cannot open the vault, says so plainly (and how to fix it), and never shows the
// password, the host or the user; list_connections still shows nothing secret. A connection
// with a password command keeps working beside it.
//
// BUG QA3-MCP-08 (low, also in agents-agents-page): the Agents page keeps showing the
// vaulted connection as "Read only" with no word that agents cannot open it while the
// master password is on. Expected: a note on the page (or on the row).
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { connectionToml, needs } from "../safety-helpers.mjs";
import { noMcp, startClient } from "../mcp-client.mjs";

export const skip = async () => noMcp() ?? (await needs("postgres")());

const VAULTED = "QA3 vaulted PG";
const COMMAND = "QA3 command PG";
const SECRET = "platypus";
const button = (scope, text) =>
  `[...document.querySelectorAll(${JSON.stringify(`${scope} button`)})].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const setLabeled = (scope, label, value) => `(() => {
  const el = [...document.querySelectorAll(${JSON.stringify(`${scope} label.field`)})]
    .find(l => l.querySelector('span')?.textContent === ${JSON.stringify(label)}).querySelector('input');
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('input', { bubbles: true }));
})()`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const accessOf = (name) =>
  `document.querySelector('.agents-connections tr[data-connection=${JSON.stringify(name)}] td:last-child')?.textContent`;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000361"
name = "${VAULTED}"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = true
${connectionToml("postgres", "00000000-0000-4000-8000-000000000362", COMMAND)}`,
  );
}

async function settings(page, tab) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab(tab);
}

async function closeSettings(page) {
  await page.eval(`${button(".modal.settings .actions", "Done")}.click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(VAULTED)} !== undefined`);
  await settings(page, "Master password");
  await page.waitFor(`document.querySelector('.master-password button') !== null`);
  await page.eval(`${button(".master-password", "Set a master password…")}.click()`);
  await page.eval(setLabeled(".master-password", "New master password", "qa3-master"));
  await page.eval(setLabeled(".master-password", "Repeat it", "qa3-master"));
  await page.eval(`${button(".master-password", "Turn on")}.click()`);
  await page.waitFor(`document.querySelector('.master-password .notice.ok') !== null`, 15_000);
  await closeSettings(page);

  await page.eval(`[...${treeRow(VAULTED)}.querySelectorAll('button')].find(b => b.title === 'Edit').click()`);
  await page.waitFor(`document.querySelector('.modal form, form.modal') !== null`);
  await page.eval(setLabeled(".modal", "Password", SECRET));
  await page.eval(`${button(".modal .actions", "Save")}.click()`);
  await page.waitFor(`document.querySelector('.modal') === null`);
  const vault = join(dir, "secrets.vault");
  assert.ok(existsSync(vault) && !readFileSync(vault, "utf8").includes(SECRET), "the password is in the encrypted file");

  const mcp = await startClient(dir, "qa3-vault");
  const problems = [];
  try {
    const list = (await mcp.call("list_connections")).text;
    assert.doesNotMatch(list, /localhost|55432|"user"|password|secret/i, list);
    for (const [tool, args] of [
      ["run_query", { connection: VAULTED, sql: "SELECT 1" }],
      ["list_schemas", { connection: VAULTED }],
      ["describe_table", { connection: VAULTED, schema: "public", table: "x" }],
    ]) {
      const r = await mcp.call(tool, args);
      assert.ok(r.error, `${tool} on a vaulted connection: ${r.text}`);
      assert.match(r.text, /master password/, `${tool}: ${r.text}`);
      assert.match(r.text, /password command|\.pgpass/, "says how to fix it");
      assert.ok(!r.text.includes(SECRET) && !r.text.includes("localhost"), `${tool} leaks: ${r.text}`);
    }
    // A connection with a password command works beside it.
    const ok = await mcp.call("run_query", { connection: COMMAND, sql: "SELECT 'reachable' AS s" });
    assert.match(ok.text, /reachable/, ok.text);

    await settings(page, "Agents");
    await page.waitFor(`${accessOf(VAULTED)} !== undefined`);
    // The note comes once the page has asked whether the vault is on: give it the time.
    await page.waitFor(`document.querySelector('.agents-panel [data-notice="vault"]') !== null`, 10_000).catch(() => {});
    const label = await page.eval(accessOf(VAULTED));
    const panel = await page.eval(`document.querySelector('.agents-panel').textContent`);
    if (!/master password/i.test(panel) && !/password|locked/i.test(label)) {
      problems.push(`QA3-MCP-08: with the master password on, the Agents page shows "${label}" for ${VAULTED} and says nothing about agents being unable to open it`);
    }
    await closeSettings(page);
  } finally {
    mcp.kill();
  }
  assert.deepEqual(problems, []);
}
