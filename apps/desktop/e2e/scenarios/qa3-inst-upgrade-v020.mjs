// Upgrade from 0.2.0: a profile left by the 0.2.0 build that shipped `platypus-mcp` (Italian,
// light theme, a project, a linked script, history and a master-password vault) opens in this
// build with everything kept: the language and theme, the project and its connections, the
// script and its link, the history, and the vault, which unlocks with the old master password
// and still holds the saved password. The log goes to logs/platypus.log and the Agents page
// points at the app itself with --mcp, not at the old platypus-mcp.exe.
import assert from "node:assert/strict";
import { cpSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { dirname, join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

const fixture = join(dirname(fileURLToPath(import.meta.url)), "..", "fixtures", "qa3-inst-v020");
const VAULT_PG = "00000000-0000-4000-8000-0000000000a1";
const invoke = (cmd, args = {}) =>
  `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`;
const labels = `[...document.querySelectorAll('.tree-row .tree-label')].map(l => l.textContent)`;

export function setup({ dir }) {
  cpSync(fixture, dir, { recursive: true, filter: (src) => !src.endsWith("README.txt") });
  const toml = join(dir, "connections.toml");
  writeFileSync(toml, readFileSync(toml, "utf8").replaceAll("{DIR}", dir));
  const lite = new DatabaseSync(join(dir, "qa.sqlite"));
  lite.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t (v) VALUES ('a');");
  lite.close();
  // The history as 0.2.0 wrote it: schema 2 (the agents' origin column).
  const history = new DatabaseSync(join(dir, "history.db"));
  history.exec(`CREATE TABLE history (
      id INTEGER PRIMARY KEY, connection_id TEXT NOT NULL, sql TEXT NOT NULL,
      started_at INTEGER NOT NULL, duration_ms INTEGER NOT NULL, rows INTEGER,
      status TEXT NOT NULL, error TEXT, origin TEXT);
    CREATE INDEX history_connection ON history (connection_id, id);
    PRAGMA user_version = 2;`);
  const insert = history.prepare(
    "INSERT INTO history (connection_id, sql, started_at, duration_ms, rows, status) VALUES (?, ?, ?, 2, 1, 'ok')",
  );
  insert.run("4e1e9d59-2781-4677-80be-4945de3c4e1d", "SELECT 3 AS qa_lite", Date.now() - 2000);
  insert.run(VAULT_PG, "SELECT 1 AS qa_pg", Date.now() - 1000);
  history.close();
}

/** Whether the Postgres container of docker/compose.yml answers. */
const postgresUp = () =>
  new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port: 55432, timeout: 1500 });
    socket.once("connect", () => (socket.destroy(), resolve(true)));
    socket.once("error", () => resolve(false));
    socket.once("timeout", () => (socket.destroy(), resolve(false)));
  });

export default async function (page, { dir }) {
  // Language and theme of the old profile.
  await page.waitFor(`document.documentElement.lang === 'it'`);
  assert.ok(await page.eval(`document.querySelector('button[title="Nuova connessione"]') !== null`));
  const settings = await page.eval(`${invoke("get_settings")}.then(s => JSON.stringify(s))`);
  assert.equal(JSON.parse(settings).appearance?.theme, "platypus_light", settings);

  // Connections and the project.
  await page.waitFor(`${labels}.includes('QA SQLite') && ${labels}.includes('Vault Postgres')`);
  const projects = JSON.parse(await page.eval(`${invoke("list_projects")}.then(p => JSON.stringify(p))`));
  assert.deepEqual(
    projects.map((p) => [p.name, p.connections.length]),
    [["QA Project", 2]],
  );

  // The vault made by 0.2.0 unlocks with its master password and keeps the password.
  const locked = JSON.parse(await page.eval(`${invoke("vault_status")}.then(s => JSON.stringify(s))`));
  assert.deepEqual(locked, { enabled: true, unlocked: false });
  const unlocked = JSON.parse(
    await page.eval(`${invoke("unlock_vault", { password: "QaMaster-1" })}.then(s => JSON.stringify(s))`),
  );
  assert.equal(unlocked.unlocked, true);
  assert.equal(await page.eval(invoke("connection_has_password", { connectionId: VAULT_PG })), true);
  if (await postgresUp()) {
    const opened = await page.eval(
      `${invoke("open_session", { connectionId: VAULT_PG, password: null, database: null, side: true })}` +
        `.then(s => 'ok ' + s.server.version, e => 'error ' + JSON.stringify(e))`,
    );
    assert.match(opened, /^ok /, opened);
  }

  // History, scripts and their link.
  const history = JSON.parse(
    await page.eval(
      `${invoke("search_history", { connectionIds: null, text: null, beforeId: null, origin: null, limit: 50 })}.then(h => JSON.stringify(h.map(e => e.sql)))`,
    ),
  );
  assert.deepEqual(history, ["SELECT 1 AS qa_pg", "SELECT 3 AS qa_lite"]);
  const scripts = JSON.parse(await page.eval(`${invoke("list_scripts", { project: null })}.then(s => JSON.stringify(s.entries))`));
  assert.deepEqual(
    scripts.map((s) => [s.path, s.connection]),
    [["qa-snippet.sql", VAULT_PG]],
  );

  // The log file and the Agents page of this build.
  const log = join(dir, "logs", "platypus.log");
  assert.ok(existsSync(log), "logs/platypus.log missing");
  assert.match(readFileSync(log, "utf8"), /PlatypusDB \S+ started/);
  const agent = JSON.parse(await page.eval(`${invoke("agent_setup")}.then(s => JSON.stringify(s))`));
  assert.match(agent.mcp_path, /platypus-db\.exe$/i, agent.mcp_path);
  assert.match(agent.claude_command, / --mcp$/, agent.claude_command);
  assert.doesNotMatch(JSON.stringify(agent), /platypus-mcp/);

  // The history store is at this build's schema, its rows kept.
  const db = new DatabaseSync(join(dir, "history.db"), { readOnly: true });
  const version = db.prepare("PRAGMA user_version").get().user_version;
  const rows = db.prepare("SELECT count(*) AS n FROM history").get().n;
  db.close();
  assert.ok(version >= 2, `history schema ${version}`);
  assert.equal(rows, 2);
}
