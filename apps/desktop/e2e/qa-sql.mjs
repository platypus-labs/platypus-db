// What the SQL editor QA scenarios (scenarios/qa-sql-*.mjs) share: connections to the
// databases of docker/compose.yml (passwords given by a command, so no prompt), a skip when
// a database is not running, and running statements in the visible query tab.
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "./cdp.mjs";
import { revealItem } from "./menu-sub.mjs";

export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
export const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
/** The editor's lines (not the SQL field above the results, also a CodeMirror). */
export const editorText = `[...${visibleTab}.querySelector('.cm-content').querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
export const resultTabs = `[...${visibleTab}.querySelectorAll('.result-tab')].map(t => t.textContent.replace(/×$/, ''))`;
export const resultError = `(${visibleTab}.querySelector('.result-error')?.textContent ?? null)`;
export const button = (label) =>
  `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === ${JSON.stringify(label)})`;
export const idle = `(${button("Cancel")}?.disabled === true)`;
export const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

const server = (name, driver, port, database, user, password, extra = "") => ({
  name,
  port,
  toml: (id) => `[[connections]]
id = "${id}"
name = "${name}"
driver = "${driver}"
host = "127.0.0.1"
port = ${port}
database = "${database}"
user = "${user}"
ssl_mode = "disable"
save_password = false
password_command = "echo ${password}"
${extra}`,
});

/** The databases of docker/compose.yml. */
export const DB = {
  pg: server("QA PG", "postgres", 55432, "platypus", "platypus", "platypus"),
  mysql: server("QA MySQL", "mysql", 53306, "platypus", "platypus", "platypus"),
  mariadb: server("QA MariaDB", "mysql", 53307, "platypus", "platypus", "platypus"),
  mssql: server("QA MSSQL", "sqlserver", 51433, "platypus", "sa", "Platypus-2026"),
  clickhouse: server("QA ClickHouse", "clickhouse", 58123, "platypus", "platypus", "platypus"),
  mongo: server("QA Mongo", "mongo", 57017, "platypus?authSource=admin", "platypus", "platypus"),
};

/** A reason to skip unless every database in `keys` accepts connections. */
export async function skipUnless(...keys) {
  for (const key of keys) {
    const { port, name } = DB[key];
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
    if (!up) return `${name}: nothing on port ${port} (docker compose -f docker/compose.yml --profile <name> up -d)`;
  }
  return null;
}

/** Saves connections to the servers in `keys`, and to a SQLite file `qa.db` made with `sqliteSql`
 *  (named "QA SQLite") and a DuckDB file (named "QA DuckDB") when asked. */
export function saveConnections(dir, keys, { sqliteSql = null, duckdb = false } = {}) {
  let n = 0;
  const id = () => `00000000-0000-4000-8000-0000000b${String(++n).padStart(4, "0")}`;
  const parts = keys.map((k) => DB[k].toml(id()));
  if (sqliteSql !== null) {
    const path = join(dir, "qa.db");
    const db = new DatabaseSync(path);
    db.exec(sqliteSql);
    db.close();
    parts.push(`[[connections]]
id = "${id()}"
name = "QA SQLite"
driver = "sqlite"
database = '${path}'
save_password = false
`);
  }
  if (duckdb) {
    parts.push(`[[connections]]
id = "${id()}"
name = "QA DuckDB"
driver = "duckdb"
database = '${join(dir, "qa.duckdb")}'
save_password = false
`);
  }
  writeFileSync(join(dir, "connections.toml"), `version = 6\n\n${parts.join("\n")}`);
}

/** Opens a new SQL editor on the connection `name` (a double click on it). */
export async function openEditor(page, name) {
  await page.waitFor(`${treeRow(name)} !== undefined`, 30_000);
  const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${treeRow(name)}.scrollIntoView({ block: 'center' }); ${treeRow(name)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-conn')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${before} && ${visibleTab} !== undefined`, 30_000);
}

/** Replaces the editor's text with `sql`, once the editor shows it; closes the completion list. */
export async function setEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`, 15_000);
  await page.key("Escape", "Escape", 27);
}

export const clearStatus = (page) =>
  page.eval(`(() => { const s = ${visibleTab}?.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);

/** Waits for the run to end (Cancel disabled, a new status) and returns the status. */
export async function waitDone(page, timeout = 30_000) {
  await page.waitFor(`(() => { const s = ${status}; return ${idle} && s !== '' && !s.includes('Executing'); })()`, timeout);
  return page.eval(status);
}

/** Runs the statement under the cursor (Ctrl+Enter) after writing `sql`, if given. */
export async function runStatement(page, sql, timeout) {
  if (sql !== undefined) await setEditor(page, sql);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  return waitDone(page, timeout);
}

/** Runs the editor's text as a script after writing `sql`, if given. */
export async function runScript(page, sql, timeout) {
  if (sql !== undefined) await setEditor(page, sql);
  await clearStatus(page);
  await page.eval(`${button("Script")}.click()`);
  return waitDone(page, timeout);
}

/** The shown result as TSV with headers (select all, copy with headers). */
export async function gridTsv(page) {
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`window.__copied = undefined`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("C", "KeyC", 67, CTRL | 8);
  await page.waitFor(`window.__copied !== undefined`, 5000);
  return page.eval(`window.__copied`);
}

/** Clicks the editor context menu entry starting with `label`. */
export async function editorMenu(page, label) {
  await page.eval(`${visibleTab}.querySelector('.cm-editor').id = 'e2e-editor'`);
  const r = await page.rect("#e2e-editor");
  await page.eval(`document.getElementById('e2e-editor').removeAttribute('id')`);
  await page.click(r.x + r.w / 2, r.y + 10, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await revealItem(page, label);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent.startsWith(${JSON.stringify(label)})).click()`);
}

/** Answers the parameters dialog with `values`, in order, and runs. */
export async function answerParams(page, values) {
  const dialog = `document.querySelector('.params-dialog')`;
  await page.waitFor(`${dialog}?.querySelector('input') != null`);
  for (let i = 0; i < values.length; i++) {
    await page.eval(`${dialog}.querySelectorAll('input')[${i}].focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    await page.type(values[i]);
  }
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Run').click()`);
}
