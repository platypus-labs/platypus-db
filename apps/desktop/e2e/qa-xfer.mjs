// What the import, export and fake data scenarios (qa-xfer-*) share: connections to the
// databases of docker/compose.yml, statements run on them through the containers' own
// clients, and the dialogs of the app driven as a user would.
import { spawnSync } from "./proc.mjs";
import { readFileSync, writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { inflateRawSync } from "node:zlib";
import { ALT, CTRL } from "./cdp.mjs";
import { revealItem } from "./menu-sub.mjs";

/** The databases: their connection in connections.toml, port and container client. */
export const DBS = {
  pg: {
    name: "QA PG",
    port: 55432,
    toml: `driver = "postgres"\nhost = "localhost"\nport = 55432\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`,
    cli: (sql) => ["platypus-db-postgres-1", ["psql", "-U", "platypus", "-d", "platypus", "-At", "-v", "ON_ERROR_STOP=1", "-c", sql]],
  },
  mysql: {
    name: "QA MySQL",
    port: 53306,
    toml: `driver = "mysql"\nhost = "127.0.0.1"\nport = 53306\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`,
    cli: (sql) => ["platypus-db-mysql-1", ["mysql", "-uplatypus", "-pplatypus", "platypus", "-N", "-B", "-e", sql]],
  },
  mariadb: {
    name: "QA MariaDB",
    port: 53307,
    toml: `driver = "mysql"\nhost = "127.0.0.1"\nport = 53307\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`,
    cli: (sql) => ["platypus-db-mariadb-1", ["mariadb", "-uplatypus", "-pplatypus", "platypus", "-N", "-B", "-e", sql]],
  },
  mssql: {
    name: "QA MSSQL",
    port: 51433,
    password: "Platypus-2026",
    toml: `driver = "sqlserver"\nhost = "127.0.0.1"\nport = 51433\ndatabase = "platypus"\nuser = "sa"\nssl_mode = "disable"`,
    cli: (sql) => [
      "platypus-db-sqlserver-1",
      ["/opt/mssql-tools18/bin/sqlcmd", "-C", "-b", "-S", "localhost", "-U", "sa", "-P", "Platypus-2026", "-d", "platypus", "-h", "-1", "-W", "-s", "\t", "-Q", `SET NOCOUNT ON; ${sql}`],
    ],
  },
  clickhouse: {
    name: "QA ClickHouse",
    port: 58123,
    toml: `driver = "clickhouse"\nhost = "127.0.0.1"\nport = 58123\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`,
    cli: (sql) => ["platypus-db-clickhouse-1", ["clickhouse-client", "--user", "platypus", "--password", "platypus", "-d", "platypus", "--multiquery", "-q", sql]],
  },
  mongo: {
    name: "QA Mongo",
    port: 57017,
    toml: `driver = "mongo"\nhost = "localhost"\nport = 57017\ndatabase = "platypus?authSource=admin"\nuser = "platypus"\nssl_mode = "disable"`,
    cli: (js) => ["platypus-db-mongo-1", ["mongosh", "--quiet", "-u", "platypus", "-p", "platypus", "--authenticationDatabase", "admin", "platypus", "--eval", js]],
  },
  cockroach: {
    name: "QA Cockroach",
    port: 56257,
    password: "",
    toml: `driver = "postgres"\nhost = "127.0.0.1"\nport = 56257\ndatabase = "platypus"\nuser = "root"\nssl_mode = "disable"`,
    cli: (sql) => ["platypus-db-cockroach-1", ["cockroach", "sql", "--insecure", "-d", "platypus", "--format=tsv", "-e", sql]],
  },
};

/** Runs `sql` (JavaScript for MongoDB) in the container of `db`; returns its output, trimmed.
 *  Throws with the client's message when it fails. */
export function db(kind, sql) {
  const [container, args] = DBS[kind].cli(sql);
  const r = spawnSync("docker", ["exec", container, ...args], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${kind}: ${r.stderr || r.stdout}`);
  return r.stdout.replace(/\r/g, "").trim();
}

/** Like `db`, ignoring failures (cleanup). */
export function dbQuiet(kind, sql) {
  try {
    return db(kind, sql);
  } catch {
    return null;
  }
}

/** A reason to skip when one of `kinds` is not listening. */
export async function skipUnless(...kinds) {
  for (const kind of kinds) {
    const port = DBS[kind].port;
    const up = await new Promise((resolve) => {
      const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
      const done = (ok) => {
        socket.destroy();
        resolve(ok);
      };
      socket.on("connect", () => done(true));
      socket.on("error", () => done(false));
      socket.on("timeout", () => done(false));
    });
    if (!up) return `no ${kind} on port ${port} (docker compose -f docker/compose.yml up -d)`;
  }
  return null;
}

/** connections.toml with the connections `kinds` (keys of DBS) and `extra` TOML, each with
 *  its password from a command so no prompt opens. */
export function saveConnections(dir, kinds, extra = "") {
  const blocks = kinds.map((kind, i) => {
    const d = DBS[kind];
    const password = d.password ?? "platypus";
    return `
[[connections]]
id = "00000000-0000-4000-8000-0000000009${String(i).padStart(2, "0")}"
name = "${d.name}"
${d.toml}
connection_type = "dev"
save_password = false
${password ? `password_command = "echo ${password}"` : ""}
`;
  });
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${blocks.join("")}${extra}`);
}

/** A SQLite connection block for `saveConnections`' extra. */
export const sqliteBlock = (name, path, id = "00000000-0000-4000-8000-000000000990", type = "dev") => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlite"
database = '${path}'
connection_type = "${type}"
save_password = false
`;

/** The SQLite database `t.db` in `dir`, made with `sql`, as the only saved connection "QA SQLite". */
export async function sqliteOnly(dir, sql = "") {
  const { DatabaseSync } = await import("node:sqlite");
  const db = new DatabaseSync(join(dir, "t.db"));
  if (sql) db.exec(sql);
  db.close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock("QA SQLite", join(dir, "t.db"))}`);
}

/** A DuckDB connection block for `saveConnections`' extra. */
export const duckdbBlock = (name, path, id = "00000000-0000-4000-8000-000000000991") => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "duckdb"
database = '${path}'
connection_type = "dev"
save_password = false
`;

/** The tree row labelled `label`: the first one after the rows `within` (a label or a chain
 *  of labels, e.g. [connection, schema]), each found after the one before. */
export const row = (label, within = null) => {
  const chain = [within].flat().filter(Boolean);
  return `(() => {
    const rows = [...document.querySelectorAll('.tree-row')];
    const text = (r) => r.querySelector('.tree-label')?.textContent;
    let at = -1;
    for (const w of ${JSON.stringify(chain)}) {
      at = rows.findIndex((r, i) => i > at && text(r) === w);
      if (at < 0) return undefined;
    }
    return rows.find((r, i) => i > at && text(r) === ${JSON.stringify(label)});
  })()`;
};

/** Expands the tree down to `table` of `schema` (null when the connection has none) of
 *  connection `conn`, opening the schema and its Tables (or Collections) folder as needed. */
export async function showTable(page, conn, schema, table, timeout = 30_000) {
  await expand(page, conn, null);
  const chain = [conn, schema].filter(Boolean);
  const deadline = Date.now() + timeout;
  const open = async (label, within) => {
    if (await page.eval(`${row(label, within)} !== undefined && ${row(label, within)}.getAttribute('aria-expanded') !== 'true'`)) {
      await page.eval(`${row(label, within)}.querySelector('.twisty').click()`);
    }
  };
  while (!(await page.eval(`${row(table, chain)} !== undefined`))) {
    if (Date.now() > deadline) throw new Error(`no tree row ${chain.join(" > ")} > ${table}`);
    if (schema) await open(schema, conn);
    await open("Tables", chain);
    await open("Collections", chain);
    await page.sleep(300);
  }
}
export const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
export const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
export const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
export const importDialog = `document.querySelector('.modal.import-data')`;
export const generateDialog = `document.querySelector('.modal.generate-data')`;
export const exportTablesDialog = `document.querySelector('.modal.export-tables')`;
export const migrateDialog = `document.querySelector('.modal.migrate')`;

/** Expands the tree row `label` (after `within`, if given) and waits for `child`. */
export async function expand(page, label, child, within = null, timeout = 20_000) {
  await page.waitFor(`${row(label, within)} !== undefined`, timeout);
  if (await page.eval(`${row(label, within)}.getAttribute('aria-expanded') !== 'true'`)) {
    await page.eval(`${row(label, within)}.querySelector('.twisty').click()`);
  }
  if (child) await page.waitFor(`${row(child, label)} !== undefined`, timeout);
}

/** Opens the context menu of tree row `label` and clicks the item starting with `item`; waits
 *  for `modal` when given. */
export async function menu(page, label, item, modal = null, within = null) {
  await page.waitFor(`${row(label, within)} !== undefined`, 20_000);
  await page.eval(`${row(label, within)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.eval(`document.getElementById('e2e-row').scrollIntoView({ block: 'center' })`);
  await page.sleep(100);
  const r = await page.rect("#e2e-row");
  const inView = await page.eval(`(() => { const r = document.getElementById('e2e-row').getBoundingClientRect(); const e = document.elementFromPoint(r.x + 10, r.y + r.height / 2); return !!e && document.getElementById('e2e-row').closest('.tree-row').contains(e); })()`);
  if (inView) await page.click(r.x + 10, r.y + r.h / 2, { button: "right" });
  else {
    await page.eval(`(() => { const el = document.getElementById('e2e-row'); const r = el.getBoundingClientRect(); el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.x + 10, clientY: Math.min(r.y + 5, innerHeight - 200), button: 2 })); })()`);
  }
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await revealItem(page, item);
  const items = await page.eval(`[...document.querySelectorAll('.context-menu button')].map(b => b.textContent)`);
  const found = await page.eval(
    `(() => { const b = [...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})); if (b) b.click(); return !!b; })()`,
  );
  if (!found) throw new Error(`no menu item ${item} in ${JSON.stringify(items)}`);
  if (modal) await page.waitFor(`${modal} !== null`, 15_000);
}

/** Sets the React-controlled input or select `el` to `value`, as typing or choosing would. */
export const fill = (el, value) => `(() => {
  const el = ${el};
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
})()`;

/** Types `path` into the import dialog's file field and reads it, as Enter does; waits for the
 *  preview or an error. Returns the error text, or null. */
export async function chooseFile(page, path) {
  await page.waitFor(`${importDialog}?.querySelector('input') != null`);
  // The dialog opens its session first; Enter does nothing before.
  await page.waitFor(`!${button(importDialog, "Choose file…")}?.disabled`, 20_000);
  await page.eval(fill(`${importDialog}.querySelector('input')`, path));
  await page.waitFor(`${importDialog}.querySelector('input').value === ${JSON.stringify(path)}`);
  await page.eval(`${importDialog}.querySelector('input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  await page.waitFor(`${importDialog}.querySelector('.import-preview') !== null || ${importDialog}.querySelector('.notice.error') !== null`, 30_000);
  return page.eval(`${importDialog}.querySelector('.notice.error')?.textContent ?? null`);
}

/** In the open "Copy to a database" dialog, points the copy at connection `name` and waits
 *  for its schemas and the preview with the target's types. Returns the error shown, or null. */
export async function copyTarget(page, name) {
  const error = `${importDialog}.querySelector('.notice.error')`;
  await page.waitFor(`${importDialog}?.querySelector('select') != null`);
  const id = await page.eval(`[...${importDialog}.querySelector('select').options].find(o => o.textContent === ${JSON.stringify(name)})?.value`);
  if (id === undefined) throw new Error(`no target connection ${name}`);
  if (await page.eval(`${importDialog}.querySelector('select').value !== ${JSON.stringify(id)}`)) {
    await page.eval(fill(`${importDialog}.querySelector('select')`, id));
  }
  await page.waitFor(`${importDialog}.querySelectorAll('select')[1]?.value || ${error} !== null`, 30_000);
  // The preview is read again with the target's types once its session is open.
  await page.sleep(1500);
  await page.waitFor(`${importDialog}.querySelectorAll('.import-preview tbody tr').length > 0 || ${error} !== null`, 30_000);
  return page.eval(`${error}?.textContent ?? null`);
}

/** Opens "Generate data…" on `table` (after the tree rows `within`), keeps the suggested
 *  generators, asks for `rows` rows and generates them. Returns the suggested generators and
 *  the outcome, with the dialog closed. */
export async function generate(page, table, within, rows) {
  await menu(page, table, "Generate data", generateDialog, within);
  await page.waitFor(`${generateDialog}.querySelectorAll('select').length > 0 || ${generateDialog}.querySelector('.notice.error') !== null`, 20_000);
  const kinds = await page.eval(`[...${generateDialog}.querySelectorAll('select')].map(s => s.value)`);
  const input = `${generateDialog}.querySelector('input[type=number]')`;
  await page.eval(fill(input, String(rows)));
  await page.waitFor(`${input}.value === ${JSON.stringify(String(rows))}`);
  const out = await runJob(page, generateDialog, "Generate", 120_000);
  await closeModal(page, generateDialog);
  return { kinds, out };
}

/** Clicks `action` in `modal` and waits for the job's outcome: the text of the dialog's last
 *  notice. Confirms a production guard when one opens. */
export async function runJob(page, modal, action, timeout = 60_000) {
  await page.waitFor(`${button(modal, action)} && !${button(modal, action)}.disabled`, 20_000);
  await page.eval(`${button(modal, action)}.click()`);
  const guard = `document.querySelector('.modal.guard')`;
  const end = `${modal}.querySelector('.actions button')?.textContent.trim() === 'Close' || ${modal}.querySelector('.notice.error') !== null || ${guard} !== null`;
  await page.waitFor(end, timeout);
  if (await page.eval(`${guard} !== null`)) {
    await page.eval(`${button(guard, "Run")}.click()`);
    await page.waitFor(`${modal}.querySelector('.actions button')?.textContent.trim() === 'Close' || ${modal}.querySelector('.notice.error') !== null`, timeout);
  }
  return page.eval(`[...${modal}.querySelectorAll('.notice')].map(n => n.textContent).join(' | ')`);
}

/** Closes `modal` with its Close or Cancel button. */
export async function closeModal(page, modal) {
  await page.eval(`(${button(modal, "Close")} ?? ${button(modal, "Cancel")}).click()`);
  await page.waitFor(`${modal} === null`);
}

/** Opens an editor on connection `name` (double click) and waits for it. */
export async function openEditor(page, name) {
  await page.waitFor(`${row(name)} !== undefined`, 20_000);
  const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${row(name)}.id = 'e2e-conn'`);
  await page.eval(`document.getElementById('e2e-conn').scrollIntoView({ block: 'center' })`);
  await page.sleep(100);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-conn')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${before} && ${visibleTab}?.querySelector('.cm-content') != null`, 20_000);
}

/** Replaces the editor's text with `sql` and runs it all as a script; returns the status. */
export async function runScript(page, sql, timeout = 60_000) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql.replace(/\s+/g, " ").slice(-15))}) || ${editorText}.includes(${JSON.stringify(sql.slice(-15))})`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("x", "KeyX", 88, ALT);
  const idle = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')?.disabled`;
  await page.waitFor(`${idle} && ${status} !== '' && !${status}.includes('Executing')`, timeout);
  return page.eval(status);
}

/** The text of entry `name` of the zip file at `path` (an .xlsx part), or null. */
export function zipEntry(path, name) {
  const buf = readFileSync(path);
  // The end of central directory record: its offset and entry count.
  const eocd = buf.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  let at = buf.readUInt32LE(eocd + 16);
  const count = buf.readUInt16LE(eocd + 10);
  for (let i = 0; i < count; i++) {
    const method = buf.readUInt16LE(at + 10);
    const size = buf.readUInt32LE(at + 20);
    const nameLength = buf.readUInt16LE(at + 28);
    const extra = buf.readUInt16LE(at + 30);
    const comment = buf.readUInt16LE(at + 32);
    const local = buf.readUInt32LE(at + 42);
    const entry = buf.subarray(at + 46, at + 46 + nameLength).toString("utf8");
    if (entry === name) {
      const start = local + 30 + buf.readUInt16LE(local + 26) + buf.readUInt16LE(local + 28);
      const data = buf.subarray(start, start + size);
      return (method === 8 ? inflateRawSync(data) : data).toString("utf8");
    }
    at += 46 + nameLength + extra + comment;
  }
  return null;
}

/** Answers the next system save (or open) dialog with `path`. Tauri's IPC posts each command
 *  with `fetch` to `ipc://localhost/<command>`: the dialog plugin's request is answered here,
 *  every other one goes through. */
export async function answerSave(page, path, command = "plugin:dialog|save") {
  await page.eval(`(() => {
    if (!window.__qaFetch) {
      window.__qaFetch = window.fetch;
      window.__qaAnswers = {};
      window.fetch = (input, init) => {
        const url = decodeURIComponent(typeof input === 'string' ? input : input.url);
        for (const [cmd, answer] of Object.entries(window.__qaAnswers)) {
          if (url.endsWith('/' + cmd)) {
            delete window.__qaAnswers[cmd];
            return Promise.resolve(new Response(JSON.stringify(answer), { headers: { 'Content-Type': 'application/json', 'Tauri-Response': 'ok' } }));
          }
        }
        return window.__qaFetch(input, init);
      };
    }
    window.__qaAnswers[${JSON.stringify(command)}] = ${JSON.stringify(path)};
  })()`);
}

/** Exports the visible tab's result to `path` through the Export dialog. `options`: format
 *  value, delimiter, header, table, source ("query" or "loaded"). Returns the status text. */
export async function exportResult(page, path, { format = "csv", delimiter, header, table, source = "query", compression } = {}) {
  await page.eval(`[...${visibleTab}.querySelectorAll('button.link')].find(b => b.textContent === 'Export…').click()`);
  const dialog = `[...document.querySelectorAll('.modal')].find(m => m.querySelector('h2')?.textContent === 'Export result')`;
  await page.waitFor(`${dialog} !== undefined`);
  await page.eval(fill(`${dialog}.querySelector('select')`, format));
  await page.waitFor(`${dialog}.querySelector('select').value === ${JSON.stringify(format)}`);
  if (delimiter !== undefined) await page.eval(fill(`[...${dialog}.querySelectorAll('label.field')].find(l => l.textContent.startsWith('Delimiter')).querySelector('select')`, delimiter));
  if (header === false) await page.eval(`[...${dialog}.querySelectorAll('label.check')].find(l => l.textContent.includes('Header row')).querySelector('input').click()`);
  if (table !== undefined) await page.eval(fill(`[...${dialog}.querySelectorAll('label.field')].find(l => l.textContent.startsWith('Table name')).querySelector('input')`, table));
  if (compression !== undefined) await page.eval(fill(`[...${dialog}.querySelectorAll('label.field')].find(l => l.textContent.startsWith('Compression')).querySelector('select')`, compression));
  await page.eval(`${dialog}.querySelectorAll('fieldset.choices input')[${source === "query" ? 0 : 1}].click()`);
  await answerSave(page, path);
  await page.eval(`${button(dialog, "Export…")}.click()`);
  const exporting = `[...${visibleTab}.querySelectorAll('span')].find(s => /^(Exported|Export failed|Export cancelled)/.test(s.textContent))`;
  await page.waitFor(`${exporting} !== undefined`, 60_000);
  const text = await page.eval(`${exporting}.textContent`);
  // Dismissed, so the next export's outcome is new.
  await page.eval(`${exporting}.querySelector('button')?.click()`);
  return text;
}
