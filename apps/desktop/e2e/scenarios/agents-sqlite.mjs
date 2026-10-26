// Agents: the history filters the statements agents ran; the Agents page of the Settings
// shows the commands that register the app (started with --mcp) with Claude Code and Codex,
// with Copy buttons, and the agent access of each connection; the app started that way
// answers MCP on stdio; the connection dialog sets that access, read-only at most on
// production.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const DEV = "00000000-0000-4000-8000-000000000021";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const historyTexts = `[...document.querySelectorAll('.history-entry pre')].map(p => p.textContent)`;
const accessOf = (name) =>
  `document.querySelector('.agents-connections tr[data-connection=${JSON.stringify(name)}] td:last-child')?.textContent`;

/** Sets a React-controlled select and tells React, as choosing would. */
const choose = (element, value) => `(() => {
  const el = ${element};
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;

export function setup({ dir }) {
  const path = join(dir, "agents.db");
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);");
  db.close();
  const connection = (id, name, extra) => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlite"
database = '${path}'
save_password = false
${extra}
`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 4\n${connection(DEV, "Dev db", 'agent_access = "read_write"')}${connection(
      "00000000-0000-4000-8000-000000000022",
      "Prod db",
      'connection_type = "prod"\nagent_access = "read_only"',
    )}${connection("00000000-0000-4000-8000-000000000023", "Hidden db", 'agent_access = "off"')}`,
  );
  // One statement run in the app, one by an agent.
  const history = new DatabaseSync(join(dir, "history.db"));
  history.exec(`CREATE TABLE history (
      id INTEGER PRIMARY KEY, connection_id TEXT NOT NULL, sql TEXT NOT NULL,
      started_at INTEGER NOT NULL, duration_ms INTEGER NOT NULL, rows INTEGER,
      status TEXT NOT NULL, error TEXT, origin TEXT);
    CREATE INDEX history_connection ON history (connection_id, id);
    PRAGMA user_version = 2;`);
  const insert = history.prepare(
    "INSERT INTO history (connection_id, sql, started_at, duration_ms, rows, status, origin) VALUES (?, ?, ?, 3, 1, 'ok', ?)",
  );
  insert.run(DEV, "SELECT 'from the app'", Date.now() - 2000, null);
  insert.run(DEV, "SELECT 'from an agent'", Date.now() - 1000, "claude-code");
  history.close();
}

async function history(page) {
  await page.waitFor(`${treeRow("Dev db")} !== undefined`);
  await page.eval(`${treeRow("Dev db")}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await page.eval(
    `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'History').click()`,
  );
  await page.waitFor(`${historyTexts}.length === 2`);
  assert.equal(await page.eval(`document.querySelector('.history-agent')?.textContent`), "claude-code");
  await page.eval(choose(`document.querySelector('.history-origin')`, "agent"));
  await page.waitFor(`JSON.stringify(${historyTexts}) === JSON.stringify(["SELECT 'from an agent'"])`);
  await page.eval(choose(`document.querySelector('.history-origin')`, "app"));
  await page.waitFor(`JSON.stringify(${historyTexts}) === JSON.stringify(["SELECT 'from the app'"])`);
}

async function agentsPage(page, shots) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Agents");
  await page.waitFor(`document.querySelector('.agents-panel [data-field="claude_command"]') !== null`);
  const claude = await page.eval(`document.querySelector('[data-field="claude_command"]').textContent`);
  // The app runs on a profile folder of its own, which the server is given too.
  assert.match(claude, /^claude mcp add platypus -e "PLATYPUS_DATA_DIR=.+" -- ".+platypus-db\.exe" --mcp$/);
  const codex = await page.eval(`document.querySelector('[data-field="codex_config"]').textContent`);
  assert.match(codex, /^\[mcp_servers\.platypus\]$/m);
  assert.match(codex, /^command = .+platypus-db\.exe/m);
  assert.match(codex, /^args = \["--mcp"\]$/m);
  assert.match(codex, /PLATYPUS_DATA_DIR = /);

  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`document.querySelector('[data-copy="claude_command"]').click()`);
  await page.waitFor(`document.querySelector('[data-copy="claude_command"]').textContent === 'Copied'`);
  assert.equal(await page.eval(`window.__copied`), claude);

  await page.waitFor(`${accessOf("Dev db")} === 'Read and write'`);
  assert.equal(await page.eval(accessOf("Prod db")), "Read only · production");
  assert.equal(await page.eval(accessOf("Hidden db")), "Off");
  if (shots) await page.screenshot(join(shots, "settings-agents.png"));
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

async function connectionDialog(page, dir, shots) {
  await page.eval(`[...${treeRow("Dev db")}.querySelectorAll('button')].find(b => b.title === 'Edit').click()`);
  const access = `document.querySelector('.modal [data-field="agent_access"]')`;
  await page.waitFor(`${access} !== null`);
  assert.equal(await page.eval(`${access}.value`), "read_write");
  // Production: read-only at most for agents.
  const type = `[...document.querySelectorAll('.modal label.field')].find(l => l.querySelector('span')?.textContent === 'Type').querySelector('select')`;
  await page.eval(choose(type, "prod"));
  await page.waitFor(`${access}.value === 'read_only'`);
  assert.equal(await page.eval(`${access}.querySelector('option[value="read_write"]').disabled`), true);
  assert.match(
    await page.eval(`document.querySelector('.modal [data-field="agent_access_note"]').textContent`),
    /Production connections are read-only for agents/,
  );
  if (shots) await page.screenshot(join(shots, "connection-agent-access.png"));
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Save').click()`);
  await page.waitFor(`document.querySelector('.modal') === null`);
  const saved = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.match(saved, /^version = 6/m);
  const dev = saved.split("[[connections]]").find((c) => c.includes('name = "Dev db"'));
  assert.match(dev, /connection_type = "prod"/);
  assert.match(dev, /agent_access = "read_only"/);
}

/** Starts the app as the agent would, from the Claude Code command, and asks its MCP server
 *  for the connections over stdio: one window-less process answering JSON-RPC lines. */
async function mcpServer(claude, dir) {
  const exe = /-- "(.+)" --mcp$/.exec(claude)[1];
  const server = spawn(exe, ["--mcp"], {
    env: { ...process.env, PLATYPUS_DATA_DIR: dir },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const answers = new Map();
  let buffered = "";
  server.stdout.on("data", (d) => {
    buffered += d;
    let end;
    while ((end = buffered.indexOf("\n")) >= 0) {
      const message = JSON.parse(buffered.slice(0, end));
      buffered = buffered.slice(end + 1);
      if (message.id !== undefined) answers.set(message.id, message);
    }
  });
  const send = (message) => server.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", ...message })}\n`);
  const answer = async (id) => {
    const until = Date.now() + 30_000;
    while (!answers.has(id)) {
      assert.ok(Date.now() < until, `no answer ${id} from platypus-db --mcp`);
      await new Promise((r) => setTimeout(r, 50));
    }
    return answers.get(id);
  };
  try {
    send({
      id: 1,
      method: "initialize",
      params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "e2e", version: "1" } },
    });
    assert.equal((await answer(1)).result.serverInfo.name, "platypus-mcp");
    send({ method: "notifications/initialized" });
    send({ id: 2, method: "tools/call", params: { name: "list_connections", arguments: {} } });
    const listed = (await answer(2)).result.content.map((c) => c.text).join("\n");
    assert.match(listed, /Dev db/);
    assert.doesNotMatch(listed, /Hidden db/);
  } finally {
    server.kill();
  }
}

export default async function (page, { dir, shots }) {
  await history(page);
  await agentsPage(page, shots);
  await mcpServer(await page.eval(`window.__copied`), dir);
  await connectionDialog(page, dir, shots);
}
