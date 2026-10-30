// Agents, the bridge with the open app (group N): the app serves it on a local port with a
// token, written to agent-bridge.json; a wrong token is refused. An agent started with
// --mcp shows in the top bar and lists the sessions open in the app. With "Ask before agents
// change data" on, each change on a read-write connection waits for the user's answer in a
// dialog with its statement, the estimated rows and the plan: refused (with the reason the
// agent reads), allowed once (the next one asks again) or allowed for the session (the next
// ones run without asking); queries never ask. An agent that stops takes its question with
// it. Settings › Agents shows the address and token, makes a new token, reports a port in
// use, and turns the bridge off: agents then see the app closed, and their changes are
// refused. MCP clients over HTTP reach the same tools at /mcp.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";
import { setField } from "../plans.mjs";
import { openQueryTab, run } from "../conn-helpers.mjs";
import { expand } from "../qa-nav-helpers.mjs";

const RO = "00000000-0000-4000-8000-000000000041";
const RW = "00000000-0000-4000-8000-000000000042";

const dialog = `document.querySelector('.agent-write')`;
const field = (name) => `(${dialog}?.querySelector('[data-field=${JSON.stringify(name)}]')?.textContent ?? '')`;
const answer = (verdict) => `${dialog}.querySelector('[data-answer=${JSON.stringify(verdict)}]').click()`;
const pill = `(document.querySelector('.topbar-agents')?.textContent ?? '')`;
const bridgeField = (name) => `document.querySelector('.agent-bridge [data-field=${JSON.stringify(name)}]')`;
/** Enter in the port field applies it. */
const enter = `${bridgeField("bridge_port")}.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`;

export function skip() {
  return noMcp();
}

export function setup({ dir }) {
  const path = join(dir, "bridge.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT);
    WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 20)
    INSERT INTO orders SELECT i, 'open' FROM n;`);
  db.close();
  const connection = (id, name, access) => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlite"
database = '${path}'
agent_access = "${access}"
`;
  writeFileSync(join(dir, "connections.toml"), `version = 4\n${connection(RO, "Shop", "read_only")}${connection(RW, "Scratch", "read_write")}`);
  // The runner gives the bridge a free port; this scenario wants the questions.
  writeFileSync(join(dir, "settings.toml"), "[agents]\nconfirm_writes = true\n");
}

const count = (dir, where = "1 = 1") => {
  const db = new DatabaseSync(join(dir, "bridge.db"));
  const n = db.prepare(`SELECT count(*) AS n FROM orders WHERE ${where}`).get().n;
  db.close();
  return n;
};

/** The bridge's file, once the app wrote it. */
async function bridgeFile(page, dir) {
  const file = join(dir, "agent-bridge.json");
  for (let i = 0; i < 100 && !existsSync(file); i++) await page.sleep(100);
  assert.ok(existsSync(file), "the app wrote agent-bridge.json");
  return JSON.parse(readFileSync(file, "utf8"));
}

/** Asks the open app through the bridge, as `--mcp` does. */
const post = (bridge, path, body, token = bridge.token) =>
  fetch(`http://127.0.0.1:${bridge.port}${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json", Accept: "application/json, text/event-stream" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

/** Runs `sql` on Scratch through the agent and waits for the app's question. Returns the pending
 *  answer in a pair: a promise returned as is would make the caller wait for the answer too. */
async function asked(page, mcp, sql) {
  const result = mcp.call("run_query", { connection: "Scratch", sql });
  await page.waitFor(`${dialog} !== null && ${field("statement")} === ${JSON.stringify(sql)}`, 20_000);
  return [result];
}

/** The token and the bridge's switch, in Settings › Agents. */
async function openBridgeSettings(page) {
  await page.eval(`document.querySelector('.topbar-agents')?.click()`);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("Agents");
  await page.waitFor(`${bridgeField("bridge_enabled")} !== null`);
}

async function closeSettings(page) {
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent.trim() === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

async function questions(page, dir, mcp) {
  // Refused: the agent reads the reason; nothing changed.
  let [result] = await asked(page, mcp, "UPDATE orders SET status = 'closed' WHERE id <= 5");
  assert.match(await page.eval(field("rows")), /row|estimate/);
  assert.match(await page.eval(`${dialog}.querySelector('h2').textContent`), /e2e-bridge wants to change data on Scratch/);
  await setField(page, ".agent-write [data-field=reason]", "not before the release");
  await page.eval(answer("deny"));
  let r = await result;
  assert.ok(r.error && /refused this change: not before the release/.test(r.text), r.text);
  assert.equal(count(dir, "status = 'closed'"), 0);
  await page.waitFor(`${dialog} === null`);

  // Allowed once: it runs, and the next change asks again; the reason typed is gone.
  [result] = await asked(page, mcp, "UPDATE orders SET status = 'closed' WHERE id <= 5");
  await page.eval(answer("once"));
  r = await result;
  assert.equal(r.error, false, r.text);
  assert.equal(count(dir, "status = 'closed'"), 5);
  [result] = await asked(page, mcp, "DELETE FROM orders WHERE id = 20");
  assert.equal(await page.eval(`${dialog}.querySelector('[data-field=reason]').value`), "");
  // Allowed for the session: the next changes of this agent on Scratch run unasked.
  await page.eval(answer("session"));
  assert.equal((await result).error, false);
  r = await mcp.call("run_query", { connection: "Scratch", sql: "DELETE FROM orders WHERE id = 19" });
  assert.equal(r.error, false, r.text);
  assert.equal(await page.eval(`${dialog} === null`), true, "not asked again");
  assert.equal(count(dir), 18);

  // Queries never ask, nor does a read-only connection, which refuses changes itself.
  r = await mcp.call("run_query", { connection: "Shop", sql: "SELECT count(*) FROM orders" });
  assert.equal(r.error, false, r.text);
  r = await mcp.call("run_query", { connection: "Shop", sql: "DELETE FROM orders WHERE id = 1" });
  assert.ok(r.error && /read-only/.test(r.text), r.text);
  assert.equal(await page.eval(`${dialog} === null`), true);
}

/** Another agent asks, then stops before an answer: its question goes with it. */
async function agentGoesAway(page) {
  const other = await startMcp(page.dir, "e2e-leaver");
  try {
    void other.call("run_query", { connection: "Scratch", sql: "DELETE FROM orders WHERE id = 1" }).catch(() => {});
    await page.waitFor(`${dialog} !== null && ${field("statement")} === 'DELETE FROM orders WHERE id = 1'`, 20_000);
  } finally {
    other.close();
  }
  await page.waitFor(`${dialog} === null`, 20_000);
}

export default async function (page, { dir }) {
  page.dir = dir;
  const bridge = await bridgeFile(page, dir);
  assert.equal(bridge.token.length, 64);

  // A wrong token is refused; the right one lists the sessions.
  assert.equal((await post(bridge, "/v1/sessions", undefined, "nope")).status, 401);
  await expand(page, "Scratch", "main");
  const sessions = await (await post(bridge, "/v1/sessions")).json();
  assert.ok(sessions.some((s) => s.connection === "Scratch" && s.purpose === "side"), JSON.stringify(sessions));

  // An agent shows in the top bar and sees the sessions open in the app.
  const mcp = await startMcp(dir, "e2e-bridge");
  try {
    await page.waitFor(`${pill}.includes('e2e-bridge')`, 20_000);
    const open = await mcp.call("list_open_sessions");
    assert.equal(open.error, false, open.text);
    assert.ok(JSON.parse(open.text).some((s) => s.connection === "Scratch"), open.text);

    await questions(page, dir, mcp);
    await agentGoesAway(page);

    // The navigator browses on a side session; running a script in a tab opens the main one.
    await openQueryTab(page, "Scratch");
    await run(page, "SELECT 1");
    const withTab = await (await post(bridge, "/v1/sessions")).json();
    assert.ok(withTab.some((s) => s.connection === "Scratch" && s.purpose === "main"), JSON.stringify(withTab));

    // Settings › Agents: the address and the token, as the file has them.
    await openBridgeSettings(page);
    await page.waitFor(`${bridgeField("bridge_url")} !== null && ${bridgeField("bridge_token")} !== null`, 10_000);
    assert.match(await page.eval(`${bridgeField("bridge_url")}.textContent`), new RegExp(`127\\.0\\.0\\.1:${bridge.port}/mcp`));
    assert.equal(await page.eval(`${bridgeField("bridge_token")}.textContent`), bridge.token);
    assert.match(await page.eval(`${bridgeField("claude_http_command")}.textContent`), /claude mcp add --transport http platypus/);

    // A new token, after asking: the old one is refused.
    await page.eval(`document.querySelector('[data-action=regenerate_token]').click()`);
    await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
    await page.eval(`document.querySelector('.modal.confirm button.primary').click()`);
    await page.waitFor(`${bridgeField("bridge_token")}?.textContent !== ${JSON.stringify(bridge.token)}`);
    // The bridge restarts with the new token: the file is rewritten a moment after the screen shows it.
    let renewed = bridge;
    for (let i = 0; i < 100 && renewed.token === bridge.token; i++) {
      await page.sleep(100);
      renewed = await bridgeFile(page, dir);
    }
    assert.notEqual(renewed.token, bridge.token);
    assert.equal((await post(bridge, "/v1/sessions")).status, 401);
    assert.equal((await post(renewed, "/v1/sessions")).status, 200);

    // The MCP endpoint over HTTP.
    const init = await post(renewed, "/mcp", {
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "e2e-http", version: "1" } },
    });
    assert.equal(init.status, 200);
    assert.match(await init.text(), /platypus-mcp/);

    // A port in use is reported; the free one again starts it.
    const taken = createServer();
    await new Promise((ok) => taken.listen(0, "127.0.0.1", ok));
    const busy = taken.address().port;
    try {
      await setField(page, ".agent-bridge [data-field=bridge_port]", String(busy));
      await page.eval(enter);
      await page.waitFor(`${bridgeField("bridge_error")}?.textContent.includes(${JSON.stringify(String(busy))})`, 10_000);
      assert.equal(existsSync(join(dir, "agent-bridge.json")), false, "no file while it does not run");
    } finally {
      taken.close();
    }
    await setField(page, ".agent-bridge [data-field=bridge_port]", String(renewed.port));
    await page.eval(enter);
    await page.waitFor(`${bridgeField("bridge_url")} !== null`, 10_000);

    // Off: the file goes, agents see the app closed and their changes are refused.
    await page.eval(`${bridgeField("bridge_enabled")}.click()`);
    await page.waitFor(`${bridgeField("bridge_url")} === null`);
    assert.equal(existsSync(join(dir, "agent-bridge.json")), false);
    assert.match(readFileSync(join(dir, "settings.toml"), "utf8"), /bridge = false/);
    await closeSettings(page);
    await page.waitFor(`document.querySelector('.topbar-agents') === null`);
    let r = await mcp.call("list_open_sessions");
    assert.ok(r.error && /not open/.test(r.text), r.text);
    // The agent above was allowed for its session earlier, and stays so; one that starts now was not.
    const late = await startMcp(dir, "e2e-late");
    try {
      r = await late.call("run_query", { connection: "Scratch", sql: "DELETE FROM orders WHERE id = 2" });
      assert.ok(r.error && /cannot ask: PlatypusDB is not open/.test(r.text), r.text);
    } finally {
      late.close();
    }
    assert.equal(count(dir), 18);
  } finally {
    mcp.close();
  }
}
