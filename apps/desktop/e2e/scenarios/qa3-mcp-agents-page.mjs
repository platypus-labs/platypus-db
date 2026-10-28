// QA3 MCP from the app's side: the user opens Settings > Agents, copies the Claude Code
// command and the Codex table, and an MCP client started exactly as they say (path and
// PLATYPUS_DATA_DIR taken from the copied text) sees the connections and the access the
// page shows. The user then turns a connection's agent access off and later to read and
// write in the connection dialog: the running server follows at the next call, the page
// shows the new access, and a row the agent inserts is what the app's editor reads.
//
// BUG QA3-MCP-08 (low): the Agents page lists as usable connections that agents cannot
// use: one that asks for its password at each connection (no saved password, no command,
// no .pgpass) shows "Read only" like the others, while every agent call on it fails with
// "asks for its password at each connection, and agents cannot type it". Expected: the
// page says so next to it (as it says "production"), so the user fixes it before an agent
// fails. Suspected: apps/desktop/src/agents/AgentsPanel.tsx (row label only looks at the
// access and the type).
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { openEditor, runOrGuard } from "../qa-safe-db.mjs";
import { noMcp, sqliteToml, startClient } from "../qa3-mcp-client.mjs";

export const skip = noMcp;

const LITE = "QA3 agent lite";
const ASKS = "QA3 asks password";
const accessOf = (name) =>
  `document.querySelector('.agents-connections tr[data-connection=${JSON.stringify(name)}] td:last-child')?.textContent`;
const choose = (element, value) => `(() => {
  const el = ${element};
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const file = join(dir, "agent.db");
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b');");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n${sqliteToml("00000000-0000-4000-8000-000000000351", LITE, file)}
[[connections]]
id = "00000000-0000-4000-8000-000000000352"
name = "${ASKS}"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
`,
  );
}

async function openAgents(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Agents");
  await page.waitFor(`document.querySelector('.agents-panel [data-field="claude_command"]') !== null`);
  await page.waitFor(`${accessOf(LITE)} !== undefined`);
}

async function closeSettings(page) {
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

/** The server path and profile folder from the copied Claude Code command and Codex table. */
function parseCommands(claude, codex) {
  const m = /^claude mcp add platypus -e "PLATYPUS_DATA_DIR=(.+?)" -- "(.+)"$/.exec(claude);
  assert.ok(m, `unexpected Claude Code command: ${claude}`);
  const [, dataDir, exe] = m;
  // The Codex table names the same program and folder (TOML strings: literal or basic).
  const value = (key) => {
    const line = codex.split("\n").find((l) => l.startsWith(`${key} = `));
    assert.ok(line, `${key} missing from the Codex table: ${codex}`);
    const raw = line.slice(key.length + 3).trim();
    return raw.startsWith("'") ? raw.slice(1, -1) : JSON.parse(raw);
  };
  assert.equal(value("command"), exe, "Claude Code and Codex start the same program");
  assert.equal(value("PLATYPUS_DATA_DIR"), dataDir, "and give it the same profile");
  return { exe, dataDir };
}

async function editAccess(page, name, access) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`[...${treeRow(name)}.querySelectorAll('button')].find(b => b.title === 'Edit').click()`);
  const select = `document.querySelector('.modal [data-field="agent_access"]')`;
  await page.waitFor(`${select} !== null`);
  await page.eval(choose(select, access));
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Save').click()`);
  await page.waitFor(`document.querySelector('.modal') === null`);
}

export default async function (page, { dir }) {
  await openAgents(page);
  const claude = await page.eval(`document.querySelector('[data-field="claude_command"]').textContent`);
  const codex = await page.eval(`document.querySelector('[data-field="codex_config"]').textContent`);
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`document.querySelector('[data-copy="codex_config"]').click()`);
  await page.waitFor(`document.querySelector('[data-copy="codex_config"]').textContent === 'Copied'`);
  assert.equal(await page.eval(`window.__copied`), codex, "Copy puts the Codex table on the clipboard");
  const { exe, dataDir } = parseCommands(claude, codex);
  assert.equal(dataDir, dir, "the commands name the app's profile");
  const shown = {
    [LITE]: await page.eval(accessOf(LITE)),
    [ASKS]: await page.eval(accessOf(ASKS)),
  };
  assert.equal(shown[LITE], "Read only");
  await closeSettings(page);

  const mcp = await startClient(dataDir, "qa3-agents-page", { exe });
  const problems = [];
  try {
    const list = JSON.parse((await mcp.call("list_connections")).text);
    assert.deepEqual(
      list.map((c) => `${c.name}: ${c.agent_access}`).sort(),
      [`${LITE}: read_only`, `${ASKS}: read_only`].sort(),
      "the server sees what the page shows",
    );
    // QA3-MCP-08: the page shows the password-asking connection as usable.
    const asks = await mcp.call("run_query", { connection: ASKS, sql: "SELECT 1" });
    assert.ok(asks.error && /asks for its password/.test(asks.text), asks.text);
    if (!/password/i.test(shown[ASKS])) {
      problems.push(`QA3-MCP-08: the Agents page shows "${shown[ASKS]}" for ${ASKS}, which agents cannot use (${asks.text})`);
    }

    // Off in the dialog: the running server stops listing it at the next call.
    await editAccess(page, LITE, "off");
    let names = JSON.parse((await mcp.call("list_connections")).text).map((c) => c.name);
    assert.ok(!names.includes(LITE), `still listed after Off: ${names}`);
    assert.match((await mcp.call("run_query", { connection: LITE, sql: "SELECT 1" })).text, /no connection named|not available/);
    await openAgents(page);
    assert.equal(await page.eval(accessOf(LITE)), "Off");
    await closeSettings(page);

    // Read and write: the agent's insert is what the app reads.
    await editAccess(page, LITE, "read_write");
    names = JSON.parse((await mcp.call("list_connections")).text);
    assert.equal(names.find((c) => c.name === LITE)?.agent_access, "read_write");
    const insert = await mcp.call("run_query", { connection: LITE, sql: "INSERT INTO t VALUES (3, 'from the agent')" });
    assert.equal(insert.error, false, insert.text);
    await openEditor(page, LITE);
    assert.match(await runOrGuard(page, "SELECT * FROM t"), /\b3 rows\b/);
    const saved = readFileSync(join(dir, "connections.toml"), "utf8");
    assert.match(saved.split("[[connections]]").find((c) => c.includes(LITE)), /agent_access = "read_write"/);
    // The agent's statement is in the history, with the client's name.
    const history = new DatabaseSync(join(dir, "history.db"));
    const origins = history.prepare("SELECT origin FROM history WHERE sql LIKE 'INSERT INTO t%'").all().map((r) => r.origin);
    history.close();
    assert.deepEqual(origins, ["qa3-agents-page"]);
  } finally {
    mcp.kill();
  }
  assert.deepEqual(problems, []);
}
