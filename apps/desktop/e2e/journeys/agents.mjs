// Journey (docs/COLLAUDO.md 5.6): an agent through the app's MCP server, played by a JSON-RPC
// client, on a PostgreSQL connection the user makes in the app (its password goes to the
// keychain, never to a response). Settings > Agents shows the command that registers the
// server; the agent lists the connection and reads a table; its write is refused while the
// access is "Read only"; the user allows writes in the connection dialog and the next write
// asks in the app (refused once with a reason, then allowed) and runs; the user marks the
// connection Production and the write is refused again; every call is in the log of the data
// folder; after a restart of the app and the client the server works again with the same
// profile. Run in English, in Italian and by keyboard.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { keys, press, setLanguage } from "../journey.mjs";
import { noMcp, startMcp } from "../mcp.mjs";
import { needsPostgres, psql, psqlTry, restart } from "../qa-jrn.mjs";
import { ui } from "../journey-work.mjs";
import { dialog, formField, picker } from "../conn-helpers.mjs";

export const env = ({ dir }) => ({ PLATYPUS_CLIENTS_HOME: join(dir, "home") });
export const skip = async () => noMcp() ?? (await needsPostgres());
const NAME = "S jrn agent";
const URL = "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable";
const update = (v, id) => `UPDATE s_jrn_agent SET v = '${v}' WHERE id = ${id}`;

export function setup({ dir }, mode) {
  psql("SET lock_timeout = '10s'; DROP TABLE IF EXISTS s_jrn_agent; CREATE TABLE s_jrn_agent (id int PRIMARY KEY, v text); INSERT INTO s_jrn_agent VALUES (1, 'a'), (2, 'b');");
  // The runner leaves the questions off; this journey wants them.
  writeFileSync(join(dir, "settings.toml"), "[agents]\nconfirm_writes = true\n");
  setLanguage(dir, mode);
}

/** The calls in the log of the data folder: tool and arguments, oldest first. */
function loggedCalls(dir) {
  const db = new DatabaseSync(join(dir, "agent-calls.db"));
  try {
    const table = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").all()[0].name;
    return db.prepare(`SELECT * FROM "${table}" ORDER BY rowid`).all().map((r) => ({ ...r }));
  } finally {
    db.close();
  }
}

/** Chooses Host or URL in the dialog: a click, or the arrows of the radio group (one Tab stop). */
async function connectBy(page, u, mode) {
  const option = `document.querySelector('.connection-dialog [data-field="connect_by_${mode}"]')`;
  if (!u.kb) return u.tap(option);
  await u.tabTo(`document.querySelector('.connection-dialog .connect-by [aria-checked=true]')`);
  await press(page, mode === "url" ? "ArrowRight" : "ArrowLeft", mode === "url" ? "ArrowRight" : "ArrowLeft", mode === "url" ? 39 : 37);
  await page.waitFor(`document.activeElement === ${option} && ${option}.getAttribute('aria-checked') === 'true'`, 5_000);
}

/** Makes the connection from a URL, as a user does in the connection dialog. */
async function newConnection(page, u) {
  const t = u.t;
  await u.tap(`document.querySelector('.sidebar-header button[aria-label=${JSON.stringify(t("New connection"))}]')`);
  await page.waitFor(`${picker} !== null`, 15_000);
  await page.waitFor(`[...document.querySelectorAll('.driver-tile')].some(b => b.textContent === 'PostgreSQL')`, 15_000);
  if (u.kb) {
    const all = await page.eval(`document.querySelectorAll('.driver-tile').length`);
    await page.type("PostgreSQL");
    // The search leaves PostgreSQL and its variants.
    await page.waitFor(`[...document.querySelectorAll('.driver-tile')].filter(b => b.getClientRects().length).length < ${all}`, 5_000);
  }
  await u.tap(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === 'PostgreSQL')`);
  await page.waitFor(`${dialog} !== null`, 15_000);
  await u.italian("new connection form", ["PostgreSQL", "localhost", "platypus"]);
  await connectBy(page, u, "url");
  await u.type(`document.querySelector('.connection-dialog [data-field="connection_url"]')`, URL);
  // Once read, the field may show the URL without its password, which moves to the Password field.
  await page.waitFor(`document.querySelector('.connection-dialog [data-field="connection_url"]').value.endsWith('@localhost:55432/platypus?sslmode=disable')`);
  await connectBy(page, u, "host");
  await page.waitFor(`${formField(t("Host"))}?.value === 'localhost'`, 10_000);
  await u.type(formField(t("Name")), NAME);
  await page.waitFor(`${formField(t("Name"))}.value === ${JSON.stringify(NAME)}`);
  await u.tap(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(t("Save"))})`);
  await page.waitFor(`${dialog} === null`, 15_000);
  await page.waitFor(`[...document.querySelectorAll('.tree-label')].some(l => l.textContent === ${JSON.stringify(NAME)})`);
}

/** Opens the connection's dialog on its page "Appearance and safety" and sets `field` to `value`. */
async function setSafety(page, u, field, value) {
  const t = u.t;
  await u.rowMenu([NAME], [t("Edit…")]);
  await page.waitFor(`${dialog} !== null`, 15_000);
  await u.selectTab(`[...${dialog}.querySelectorAll('[role=tab]')].find(b => b.textContent.startsWith(${JSON.stringify(t("Appearance and safety"))}))`);
  await page.waitFor(`document.querySelector('.connection-dialog [data-page=safety]:not([hidden])') !== null`);
  await u.italian("connection dialog, safety page", ["PostgreSQL", "localhost"]);
  await u.pick(field, value);
  await u.tap(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(t("Save"))})`);
  await page.waitFor(`${dialog} === null`, 15_000);
}

export async function run(page, ctx, mode) {
  const u = ui(page, mode);
  const t = u.t;
  const responses = [];
  let mcp;
  try {
    await page.waitFor(`document.querySelector('.sidebar-header') !== null`);
    await u.italian("main screen");

    // Settings > Agents: the command that registers the server, copied.
    await page.key(",", "Comma", 188, CTRL);
    await page.waitFor(`document.querySelector('.modal.settings') !== null`, 10_000);
    await u.selectTab(`document.querySelector('.modal.settings [role="tab"][data-section="Agents"]')`);
    await page.waitFor(`document.querySelector('.agents-panel [data-client="claude_code"]') !== null`, 10_000);
    await u.tap(`document.querySelector('[data-client="claude_code"]')`);
    await page.waitFor(`document.querySelector('[data-field="claude_command"]') !== null`);
    await u.italian("Settings, Agents", ["claude", "PLATYPUS_DATA_DIR", "Claude Code", "Codex", "Cursor", "Windsurf", "Claude Desktop", "VS Code", "Gemini CLI", "Zed"]);
    const command = await page.eval(`document.querySelector('[data-field="claude_command"]').textContent`);
    const m = /^claude mcp add -s user platypus -e "PLATYPUS_DATA_DIR=(.+?)" -- "(.+)" --mcp$/.exec(command);
    assert.ok(m, `the proposed client command: ${command}`);
    assert.equal(m[1], ctx.dir, "the command names the app's profile");
    assert.equal(m[2].toLowerCase().replaceAll("/", "\\"), ctx.exe.toLowerCase().replaceAll("/", "\\"), "and the app itself, started with --mcp");
    await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
    await u.tap(`document.querySelector('[data-copy="claude_command"]')`);
    await page.waitFor(`window.__copied === ${JSON.stringify(command)}`, 5_000);
    await u.dismiss(`document.querySelector('.modal.settings')`, t("Done"));

    // The connection is made in the app; its password stays in the keychain.
    await newConnection(page, u);
    assert.doesNotMatch(readFileSync(join(ctx.dir, "connections.toml"), "utf8"), /platypus:platypus|^password\s*=/m, "the password stays out of the file");

    // The agent: lists the connection, reads the table.
    mcp = await startMcp(ctx.dir, "s-jrn-agent");
    const run = async (sql) => {
      const r = await mcp.call("run_query", { connection: NAME, sql });
      responses.push(r.text);
      return r;
    };
    const list = await mcp.call("list_connections");
    responses.push(list.text);
    assert.deepEqual(JSON.parse(list.text).map((c) => [c.name, c.agent_access]), [[NAME, "read_only"]]);
    const read = await run("SELECT * FROM s_jrn_agent ORDER BY id");
    assert.equal(read.error, false, read.text);
    assert.match(read.text, /\(2 rows/);
    assert.match(read.text, /\ba\b[\s\S]*\bb\b/);

    // A write with read-only access is refused.
    const refused = await run(update("x", 1));
    assert.ok(refused.error && /read-only for agents/.test(refused.text), refused.text);
    assert.equal(psql("SELECT v FROM s_jrn_agent WHERE id = 1"), "a");

    // The user allows writes: the next write asks in the app.
    await setSafety(page, u, `document.querySelector('.connection-dialog [data-field="agent_access"]')`, "read_write");
    const dialogOf = `document.querySelector('.agent-write')`;
    const field = (name) => `(${dialogOf}?.querySelector('[data-field=${JSON.stringify(name)}]')?.textContent ?? '')`;
    let pending = run(update("y", 1));
    await page.waitFor(`${dialogOf} !== null && ${field("statement")} === ${JSON.stringify(update("y", 1))}`, 30_000);
    await u.italian("the agent's question", ["s_jrn_agent", "s-jrn-agent", NAME]);
    assert.match(await page.eval(`${dialogOf}.querySelector('h2').textContent`), /s-jrn-agent/);
    await u.type(`${dialogOf}.querySelector('[data-field=reason]')`, "not now");
    await u.tap(`${dialogOf}.querySelector('[data-answer="deny"]')`);
    const denied = await pending;
    assert.ok(denied.error && /not now/.test(denied.text), denied.text);
    assert.equal(psql("SELECT v FROM s_jrn_agent WHERE id = 1"), "a", "the refused write changed nothing");
    await page.waitFor(`${dialogOf} === null`);
    pending = run(update("y", 1));
    await page.waitFor(`${dialogOf} !== null && ${field("statement")} === ${JSON.stringify(update("y", 1))}`, 30_000);
    await u.tap(`${dialogOf}.querySelector('[data-answer="once"]')`);
    const written = await pending;
    assert.equal(written.error, false, written.text);
    assert.equal(psql("SELECT v FROM s_jrn_agent WHERE id = 1"), "y");

    // The user marks it Production: read only for agents again.
    await setSafety(page, u, formField(t("Environment")), "prod");
    const capped = await run(update("z", 2));
    assert.ok(capped.error && /read-only for agents/.test(capped.text), capped.text);
    assert.equal(psql("SELECT string_agg(id || v, ',' ORDER BY id) FROM s_jrn_agent"), "1y,2b");
    assert.deepEqual(JSON.parse((await mcp.call("list_connections")).text).map((c) => c.agent_access), ["read_only"]);

    // Every call is in the log of the data folder, and none shows the password.
    const calls = loggedCalls(ctx.dir);
    const statements = JSON.stringify(calls);
    for (const sql of [update("x", 1), update("y", 1), update("z", 2), "SELECT * FROM s_jrn_agent ORDER BY id"]) {
      assert.ok(statements.includes(sql.replaceAll("'", "\\u0027")) || statements.includes(sql), `the log has ${sql}`);
    }
    assert.ok(calls.length >= 6, `${calls.length} calls in the log`);
    assert.doesNotMatch(statements, /platypus:platypus@/);
    for (const text of responses) assert.doesNotMatch(text, /platypus:platypus|password\s*[:=]/i, "no response shows the password");

    // The app and the client restart: the server works again with the same profile.
    mcp.close();
    mcp = null;
    await restart(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
    await u.italian("main screen after the restart");
    mcp = await startMcp(ctx.dir, "s-jrn-agent-2");
    const again = JSON.parse((await mcp.call("list_connections")).text);
    assert.deepEqual(again.map((c) => [c.name, c.agent_access]), [[NAME, "read_only"]]);
    const reread = await mcp.call("run_query", { connection: NAME, sql: "SELECT v FROM s_jrn_agent WHERE id = 1" });
    assert.equal(reread.error, false, reread.text);
    assert.match(reread.text, /\by\b/);
    assert.ok(loggedCalls(ctx.dir).length > calls.length, "the new calls are in the same log");
    void keys;
  } finally {
    mcp?.close();
    psqlTry("SET lock_timeout = '10s'; DROP TABLE IF EXISTS s_jrn_agent");
  }
}
