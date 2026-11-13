// Upgrade from 0.1.0: the profile that release wrote (connections.toml version 1 with its
// `folder` key, history at schema 1 without the agents' origin column, no settings, scripts
// or catalog) opens in this build: the connection is listed and opens, the old history is
// shown, the history store moves to this build's schema keeping its rows, and the first save
// rewrites connections.toml at the current version, agents left to read only.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const LEGACY = "00000000-0000-4000-8000-0000000000b1";
const invoke = (cmd, args = {}) =>
  `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`;
const labels = `[...document.querySelectorAll('.tree-row .tree-label')].map(l => l.textContent)`;

export function setup({ dir }) {
  const lite = new DatabaseSync(join(dir, "legacy.sqlite"));
  lite.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1), (2);");
  lite.close();
  // As 0.1.0's ConnectionStore saved it.
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "${LEGACY}"
name = "Legacy SQLite"
driver = "sqlite"
host = ""
database = '${join(dir, "legacy.sqlite")}'
user = ""
ssl_mode = "prefer"
save_password = false
read_only = false
folder = "Old folder"
`,
  );
  // 0.1.0's history: the first migration only.
  const history = new DatabaseSync(join(dir, "history.db"));
  history.exec(`CREATE TABLE history (
      id INTEGER PRIMARY KEY, connection_id TEXT NOT NULL, sql TEXT NOT NULL,
      started_at INTEGER NOT NULL, duration_ms INTEGER NOT NULL, rows INTEGER,
      status TEXT NOT NULL, error TEXT);
    CREATE INDEX history_connection ON history (connection_id, id);
    PRAGMA user_version = 1;`);
  history
    .prepare("INSERT INTO history (connection_id, sql, started_at, duration_ms, rows, status) VALUES (?, ?, ?, 1, 2, 'ok')")
    .run(LEGACY, "SELECT count(*) FROM t", Date.now() - 5000);
  history.close();
}

export default async function (page, { dir }) {
  await page.waitFor(`${labels}.includes('Legacy SQLite')`);

  const history = JSON.parse(
    await page.eval(
      `${invoke("search_history", { connectionIds: null, text: null, beforeId: null, origin: null, limit: 50 })}.then(h => JSON.stringify(h.map(e => [e.sql, e.origin ?? null])))`,
    ),
  );
  assert.deepEqual(history, [["SELECT count(*) FROM t", null]]);

  const opened = await page.eval(
    `${invoke("open_session", { connectionId: LEGACY, password: null, database: null, side: true })}` +
      `.then(s => 'ok', e => 'error ' + JSON.stringify(e))`,
  );
  assert.equal(opened, "ok");

  const db = new DatabaseSync(join(dir, "history.db"), { readOnly: true });
  const version = db.prepare("PRAGMA user_version").get().user_version;
  const columns = db.prepare("PRAGMA table_info(history)").all().map((c) => c.name);
  const rows = db.prepare("SELECT count(*) AS n FROM history").get().n;
  db.close();
  assert.ok(version >= 2, `history schema ${version}`);
  assert.ok(columns.includes("origin"), columns.join());
  assert.equal(rows, 1);

  // A save of the unchanged connection writes the current file format.
  await page.eval(
    `${invoke("list_connections")}.then(cs => window.__TAURI_INTERNALS__.invoke("save_connection", { connection: cs.find(c => c.id === ${JSON.stringify(LEGACY)}), password: { action: "keep" }, sshSecrets: {}, projects: null }))`,
  );
  const text = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.match(text, /^version = 8$/m, text);
  assert.match(text, /agent_access = "read_only"/, text);
  assert.match(text, /name = "Legacy SQLite"/, text);
}
