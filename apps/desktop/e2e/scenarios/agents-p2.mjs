// Agents, P2: the explain and sample_rows tools within the access and the limits; every tool
// call in the call log, which Settings > Agents > Activity shows by session, filters and
// exports as CSV; Setup lists the MCP clients with their configuration file and installs
// the server in one after a confirmation, with a backup of the file and its other content
// kept; a file that does not parse is left alone; Security shows each connection's access.
// The app runs with a home folder of its own, so no real client configuration is touched.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { noMcp, startMcp } from "../mcp.mjs";

const RO = "00000000-0000-4000-8000-000000000031";
const RW = "00000000-0000-4000-8000-000000000032";

const home = (dir) => join(dir, "home");
const cursorFile = (dir) => join(home(dir), ".cursor", "mcp.json");
const codexFile = (dir) => join(home(dir), ".codex", "config.toml");
const windsurfFile = (dir) => join(home(dir), ".codeium", "windsurf", "mcp_config.json");

const panel = (name) => `document.querySelector('[data-agents-panel=${JSON.stringify(name)}]')`;
const client = (id) => `document.querySelector('tr[data-client=${JSON.stringify(id)}]')`;
const callTools = `[...document.querySelectorAll('.agent-calls tr[data-call]')].map(r => r.dataset.call)`;

/** Sets a React-controlled input or select and tells React, as typing or choosing would. */
const setValue = (element, value) => `(() => {
  const el = ${element};
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
})()`;

export function skip() {
  return noMcp();
}

export function env({ dir }) {
  // The clients' files are looked up under the home folder (and %APPDATA% on Windows).
  return {
    USERPROFILE: home(dir),
    HOME: home(dir),
    APPDATA: join(home(dir), "AppData", "Roaming"),
    XDG_CONFIG_HOME: join(home(dir), ".config"),
  };
}

export function setup({ dir }) {
  const path = join(dir, "p2.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT);
    WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50)
    INSERT INTO orders SELECT i, CASE WHEN i % 2 THEN 'open' ELSE 'closed' END FROM n;
    CREATE TABLE "odd name" (x INTEGER); INSERT INTO "odd name" VALUES (7);`);
  db.close();
  const connection = (id, name, access) => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlite"
database = '${path}'
agent_access = "${access}"
`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 4\n${connection(RO, "Shop", "read_only")}${connection(RW, "Scratch", "read_write")}`,
  );
  // Cursor has a configuration with another server; Codex one with comments; Windsurf's
  // is broken.
  mkdirSync(join(home(dir), ".cursor"), { recursive: true });
  writeFileSync(cursorFile(dir), JSON.stringify({ mcpServers: { other: { command: "npx", args: ["x"] } } }));
  mkdirSync(join(home(dir), ".codex"), { recursive: true });
  writeFileSync(codexFile(dir), '# my Codex settings\nmodel = "o3"\n');
  mkdirSync(join(home(dir), ".codeium", "windsurf"), { recursive: true });
  writeFileSync(windsurfFile(dir), "{ not json");
}

/** The new tools, as an agent calls them; each call lands in the log. */
async function agentCalls(dir) {
  const mcp = await startMcp(dir, "e2e-agent");
  try {
    const tools = (await mcp.request("tools/list", {})).result.tools.map((t) => t.name);
    for (const tool of ["explain", "sample_rows"]) assert.ok(tools.includes(tool), `${tool} in ${tools}`);

    let r = await mcp.call("sample_rows", { connection: "Shop", schema: "main", table: "orders", max_rows: 3 });
    assert.ok(!r.error, r.text);
    assert.match(r.text, /^id \| status/);
    assert.match(r.text, /3 rows/);
    r = await mcp.call("sample_rows", { connection: "Shop", schema: "main", table: "odd name", format: "json" });
    assert.ok(!r.error, r.text);
    assert.deepEqual(JSON.parse(r.text).rows, [[7]]);
    r = await mcp.call("sample_rows", { connection: "Shop", schema: "main", table: "missing" });
    assert.ok(r.error && /no such table/.test(r.text), r.text);
    r = await mcp.call("sample_rows", { connection: "Nope", schema: "main", table: "orders" });
    assert.ok(r.error && /no connection named/.test(r.text), r.text);

    r = await mcp.call("explain", { connection: "Shop", sql: "SELECT * FROM orders WHERE status = 'open'" });
    assert.ok(!r.error, r.text);
    assert.match(r.text, /^-> /);
    assert.match(r.text, /Estimated plan/);
    r = await mcp.call("explain", { connection: "Shop", sql: "SELECT * FROM orders", analyze: true, format: "json" });
    assert.ok(!r.error, r.text);
    assert.equal(typeof JSON.parse(r.text).root.operation, "string");
    r = await mcp.call("explain", { connection: "Shop", sql: "DELETE FROM orders WHERE id = 1" });
    assert.ok(r.error && /read-only for agents/.test(r.text), r.text);
    r = await mcp.call("explain", { connection: "Scratch", sql: "DELETE FROM orders", analyze: true });
    assert.ok(r.error && /every row/.test(r.text), r.text);
    r = await mcp.call("explain", { connection: "Scratch", sql: "DELETE FROM orders" });
    assert.ok(!r.error, r.text);
    r = await mcp.call("explain", { connection: "Shop", sql: "SELECT 1; SELECT 2" });
    assert.ok(r.error && /one statement at a time/.test(r.text), r.text);
    r = await mcp.call("explain", { connection: "Shop", sql: "SELECT * FROM orders", max_bytes: 1 });
    assert.ok(!r.error && /left out/.test(r.text), r.text);
  } finally {
    mcp.close();
  }
  // Nothing was deleted by the explains.
  const db = new DatabaseSync(join(dir, "p2.db"));
  assert.equal(db.prepare("SELECT count(*) AS n FROM orders").get().n, 50);
  db.close();
}

async function openAgents(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Agents");
  await page.waitFor(`${panel("setup")} !== null`);
}

const showPage = async (page, name) => {
  await page.eval(`document.querySelector('[data-agents-page=${JSON.stringify(name)}]').click()`);
  await page.waitFor(`${panel(name)}.hidden === false`);
};

async function activity(page, dir, shots) {
  await showPage(page, "activity");
  await page.waitFor(`${callTools}.length === 12`);
  const tools = await page.eval(callTools);
  assert.equal(tools[0], "explain", "newest first");
  assert.equal(tools.filter((t) => t === "sample_rows").length, 4);
  assert.equal(await page.eval(`document.querySelectorAll('.agent-calls tr[data-status="error"]').length`), 5);
  const agents = await page.eval(`[...document.querySelectorAll('.agent-calls tr[data-call] td:nth-child(2)')].map(t => t.textContent)`);
  assert.ok(agents.every((a) => a === "e2e-agent"), agents.join());
  // One session, with its counts.
  const sessions = await page.eval(`[...document.querySelector('[data-filter="session"]').options].map(o => o.textContent)`);
  assert.equal(sessions.length, 2, sessions.join(" | "));
  assert.match(sessions[1], /e2e-agent · 12 calls · 5 errors/);

  await page.eval(setValue(`document.querySelector('[data-filter="status"]')`, "error"));
  await page.waitFor(`${callTools}.length === 5`);
  await page.eval(setValue(`document.querySelector('[data-filter="text"]')`, "odd name"));
  await page.waitFor(`${callTools}.length === 0`);
  assert.match(await page.eval(`document.querySelector('.agent-calls').textContent`), /No agent calls yet/);
  await page.eval(setValue(`document.querySelector('[data-filter="status"]')`, ""));
  await page.waitFor(`JSON.stringify(${callTools}) === '["sample_rows"]'`);
  // A row opens to its whole arguments.
  await page.eval(`document.querySelector('.agent-calls tr[data-call]').click()`);
  await page.waitFor(`document.querySelector('.agent-calls tr.open .agent-call-args')?.textContent.includes('"table":"odd name"')`);
  if (shots) await page.screenshot(join(shots, "agents-activity.png"));

  // The CSV the Export button saves, for the filter shown.
  const csv = await page.eval(`window.__TAURI_INTERNALS__.invoke('agent_calls_export', { query: {
    text: 'explain', session: null, status: null, before_id: null, limit: 1 } })`);
  const lines = csv.trim().split("\r\n");
  assert.equal(lines[0], "started_at,session,agent,tool,connection,arguments,duration_ms,status,error");
  assert.equal(lines.length, 1 + 8, "every matching call, not one page");
  assert.match(lines[1], /^\d{4}-\d\d-\d\dT[\d:.]+Z,[0-9a-f-]{36},e2e-agent,explain,Shop,"\{/);

  await page.eval(setValue(`document.querySelector('[data-filter="text"]')`, ""));
  await page.waitFor(`${callTools}.length === 12`);
  assert.ok(existsSync(join(dir, "agent-calls.db")));
}

/** Clicks Install for `id` and answers the confirmation with `confirm`. */
async function install(page, id, confirm) {
  await page.eval(`document.querySelector('[data-install=${JSON.stringify(id)}]').click()`);
  await page.waitFor(`document.querySelector('#confirm-message') !== null`);
  const message = await page.eval(`document.querySelector('#confirm-message').textContent`);
  const label = confirm ? "Install" : "Cancel";
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === ${JSON.stringify(label)}).click()`);
  await page.waitFor(`document.querySelector('#confirm-message') === null`);
  return message;
}

async function clients(page, dir, shots) {
  await showPage(page, "setup");
  await page.waitFor(`${client("cursor")} !== null`);
  for (const id of ["claude_desktop", "claude_code", "codex", "cursor", "windsurf", "antigravity", "gemini_cli"]) {
    assert.ok(await page.eval(`${client(id)} !== null`), id);
  }
  assert.equal(await page.eval(`${client("cursor")}.dataset.state`), "missing");
  assert.equal(await page.eval(`${client("windsurf")}.dataset.state`), "unreadable");
  assert.equal(await page.eval(`document.querySelector('[data-install="windsurf"]').disabled`), true);
  assert.match(await page.eval(`${client("cursor")}.textContent`), /\.cursor[\\/]mcp\.json/);
  // The entry to copy by hand follows the chosen client.
  await page.eval(`${client("codex")}.click()`);
  await page.waitFor(`document.querySelector('[data-field="snippet_codex"]')?.textContent.includes('[mcp_servers.platypus]')`);

  // Cancel leaves the file as it was.
  const before = readFileSync(cursorFile(dir), "utf8");
  const asked = await install(page, "cursor", false);
  assert.match(asked, /mcp\.json\? A copy of the file/);
  assert.equal(readFileSync(cursorFile(dir), "utf8"), before);

  await install(page, "cursor", true);
  await page.waitFor(`${client("cursor")}.dataset.state === 'installed'`);
  assert.match(await page.eval(`document.querySelector('[data-notice="installed"]').textContent`), /Cursor.*Backup: .*platypus-backup-/);
  const cursor = JSON.parse(readFileSync(cursorFile(dir), "utf8"));
  assert.equal(cursor.mcpServers.other.command, "npx", "other servers kept");
  assert.match(cursor.mcpServers.platypus.command, /platypus-db\.exe$/);
  assert.deepEqual(cursor.mcpServers.platypus.args, ["--mcp"]);
  assert.equal(cursor.mcpServers.platypus.env.PLATYPUS_DATA_DIR, dir);
  const backups = readdirSync(join(home(dir), ".cursor")).filter((f) => f.startsWith("mcp.json.platypus-backup-"));
  assert.equal(backups.length, 1, backups.join());
  assert.equal(readFileSync(join(home(dir), ".cursor", backups[0]), "utf8"), before);
  assert.equal(await page.eval(`document.querySelector('[data-install="cursor"]').disabled`), true);

  await install(page, "codex", true);
  await page.waitFor(`${client("codex")}.dataset.state === 'installed'`);
  const codex = readFileSync(codexFile(dir), "utf8");
  assert.ok(codex.startsWith('# my Codex settings\nmodel = "o3"\n'), codex);
  assert.match(codex, /\[mcp_servers\.platypus\]/);

  // A client without a file: it is created, with nothing to back up.
  const asks = await install(page, "gemini_cli", true);
  assert.match(asks, /^Create /);
  await page.waitFor(`${client("gemini_cli")}.dataset.state === 'installed'`);
  assert.ok(existsSync(join(home(dir), ".gemini", "settings.json")));
  assert.doesNotMatch(await page.eval(`document.querySelector('[data-notice="installed"]').textContent`), /Backup/);

  // The broken file is untouched.
  assert.equal(readFileSync(windsurfFile(dir), "utf8"), "{ not json");
  if (shots) await page.screenshot(join(shots, "agents-setup.png"));
  // The copy blocks for Claude Code and Codex are still there.
  assert.match(await page.eval(`document.querySelector('[data-field="claude_command"]').textContent`), /--mcp$/);
}

async function security(page) {
  await showPage(page, "security");
  const access = (name) =>
    page.eval(`document.querySelector('.agents-connections tr[data-connection=${JSON.stringify(name)}] td:last-child')?.textContent`);
  assert.equal(await access("Shop"), "Read only");
  assert.equal(await access("Scratch"), "Read and write");
}

async function clearLog(page) {
  await showPage(page, "activity");
  await page.waitFor(`${callTools}.length === 12`);
  await page.eval(`document.querySelector('[data-action="clear-calls"]').click()`);
  await page.waitFor(`document.querySelector('#confirm-message') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Delete').click()`);
  await page.waitFor(`${callTools}.length === 0`);
  assert.equal(await page.eval(`document.querySelector('[data-action="export-calls"]').disabled`), true);
}

export default async function (page, { dir, shots }) {
  await agentCalls(dir);
  await openAgents(page);
  await activity(page, dir, shots);
  await clients(page, dir, shots);
  await security(page);
  await clearLog(page);
}
