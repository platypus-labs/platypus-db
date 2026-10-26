// What the navigator QA scenarios (`scenarios/qa-nav-*.mjs`) share: saved connections to the
// docker/compose.yml databases, SQL run straight in their containers to seed objects, and
// driving the navigator tree (rows, context menus, the visible tab).
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { CTRL } from "./cdp.mjs";

export const visibleTab = `[...document.querySelectorAll('.query-tab, .object-tab')].find(t => t.style.display !== 'none')`;
export const rows = (label) =>
  `[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
export const row = (label) => `${rows(label)}[0]`;
/** The first row `label` below the row `parent` (deeper than it, before its next sibling). */
export const childRow = (parent, label) => `(() => {
  const all = [...document.querySelectorAll('.tree-row')];
  const at = all.findIndex(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(parent)});
  if (at < 0) return undefined;
  const level = Number(all[at].getAttribute('aria-level'));
  for (let i = at + 1; i < all.length && Number(all[i].getAttribute('aria-level')) > level; i++)
    if (all[i].querySelector('.tree-label')?.textContent === ${JSON.stringify(label)}) return all[i];
  return undefined;
})()`;
/** The labels of the tree, in order. */
export const treeLabels = `[...document.querySelectorAll('.tree-row')].map(r => r.querySelector('.tree-label')?.textContent)`;

/** The databases of docker/compose.yml: port, saved connection and the SQL client of the container. */
export const DBS = {
  postgres: {
    port: 55432,
    password: "platypus",
    connection: (name, id, extra = "") => `[[connections]]
id = "${id}"
name = "${name}"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
${extra}`,
    exec: (sql, db = "platypus") =>
      docker(["exec", "-i", "platypus-db-postgres-1", "psql", "-v", "ON_ERROR_STOP=1", "-q", "-At", "-U", "platypus", "-d", db], sql),
  },
  mysql: {
    port: 53306,
    password: "platypus",
    connection: (name, id, extra = "") => `[[connections]]
id = "${id}"
name = "${name}"
driver = "mysql"
host = "127.0.0.1"
port = 53306
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
${extra}`,
    exec: (sql) =>
      docker(["exec", "-i", "platypus-db-mysql-1", "mysql", "-uroot", "-pplatypus", "--default-character-set=utf8mb4", "-N", "-B"], sql),
    /** As the app's user, e.g. for routines it must be able to read. */
    execUser: (sql) =>
      docker(["exec", "-i", "platypus-db-mysql-1", "mysql", "-uplatypus", "-pplatypus", "--default-character-set=utf8mb4", "-N", "-B"], sql),
  },
  mariadb: {
    port: 53307,
    password: "platypus",
    connection: (name, id, extra = "") => `[[connections]]
id = "${id}"
name = "${name}"
driver = "mysql"
host = "127.0.0.1"
port = 53307
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
${extra}`,
    exec: (sql) =>
      docker(["exec", "-i", "platypus-db-mariadb-1", "mariadb", "-uroot", "-pplatypus", "--default-character-set=utf8mb4", "-N", "-B"], sql),
  },
  sqlserver: {
    port: 51433,
    password: "Platypus-2026",
    connection: (name, id, extra = "") => `[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlserver"
host = "127.0.0.1"
port = 51433
database = "platypus"
user = "sa"
ssl_mode = "disable"
save_password = false
${extra}`,
    exec: (sql) =>
      docker(
        ["exec", "-i", "platypus-db-sqlserver-1", "/opt/mssql-tools18/bin/sqlcmd", "-C", "-b", "-S", "localhost", "-U", "sa", "-P", "Platypus-2026", "-d", "platypus", "-h", "-1", "-W"],
        sql,
      ),
  },
};

function docker(args, sql) {
  const r = spawnSync("docker", args, { input: sql, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`docker ${args.slice(0, 3).join(" ")} failed: ${r.stderr || r.stdout}`);
  return r.stdout;
}

/** A skip() for a scenario that needs the database `db` running. */
export function needs(db) {
  const port = DBS[db].port;
  return () =>
    new Promise((resolve) => {
      const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
      const done = (reason) => {
        socket.destroy();
        resolve(reason);
      };
      socket.on("connect", () => done(null));
      socket.on("error", () => done(`no ${db} on port ${port} (docker compose -f docker/compose.yml up -d)`));
      socket.on("timeout", () => done(`no ${db} on port ${port}`));
    });
}

/** Saves `connections` (TOML blocks) in the profile `dir`. */
export function saveConnections(dir, ...connections) {
  writeFileSync(join(dir, "connections.toml"), `version = 1\n\n${connections.join("\n")}`);
}

/** Expands the connection `name`, answering the password prompt with `password`. */
export async function connectTo(page, name, password) {
  await page.waitFor(`${row(name)} !== undefined`);
  await page.eval(`${row(name)}.querySelector(".twisty").click()`);
  if (password === null) return;
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`, 15_000);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type(password);
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') === null`, 30_000);
}

/** Expands the row `label` (the `index`th of that label) and waits for `child` below it. */
export async function expand(page, label, child, index = 0) {
  await page.waitFor(`${rows(label)}[${index}] !== undefined`, 20_000);
  await page.eval(`${rows(label)}[${index}].querySelector(".twisty").click()`);
  if (child) await page.waitFor(`${row(child)} !== undefined`, 20_000);
}

/** Opens the context menu of the row `label` and returns its entries. */
export async function openMenu(page, label, index = 0) {
  await page.waitFor(`${rows(label)}[${index}] !== undefined`, 15_000);
  await page.eval(`${rows(label)}[${index}].scrollIntoView({ block: 'center' })`);
  await page.eval(`${rows(label)}[${index}].querySelector('.tree-label').id = 'e2e-row'`);
  const r = await page.rect("#e2e-row");
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.click(r.x + 5, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  return page.eval(`[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`);
}

/** Runs the context-menu entry `item` (its exact text, or a prefix when it ends with `*`) of the row `label`. */
export async function menu(page, label, item, index = 0) {
  await openMenu(page, label, index);
  const prefix = item.endsWith("*");
  const text = prefix ? item.slice(0, -1) : item;
  const found = `[...document.querySelectorAll('.context-menu button')].find(b => { const t = b.querySelector('span')?.textContent ?? ''; return ${prefix} ? t.startsWith(${JSON.stringify(text)}) : t === ${JSON.stringify(text)}; })`;
  const ok = await page.eval(`!!${found}`);
  if (!ok) {
    const items = await page.eval(`[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`);
    throw new Error(`no menu entry ${item} on ${label}: ${items.join(", ")}`);
  }
  await page.eval(`${found}.click()`);
}

/** The editor's text of the visible tab. */
export const editorText = `[...(${visibleTab}?.querySelector('.cm-content')?.querySelectorAll('.cm-line') ?? [])].map(l => l.textContent).join('\\n')`;
const statusSpan = `${visibleTab}?.querySelector('.statusbar span')`;
export const status = `(${statusSpan}?.textContent ?? '')`;
const cancel = `[...(${visibleTab}?.querySelectorAll('.toolbar button') ?? [])].find(b => b.getAttribute('aria-label') === 'Cancel')`;

/** Waits until the visible query tab ran: Cancel disabled and a status, after it was emptied. */
export async function waitRun(page, timeout = 20_000) {
  await page.waitFor(
    `(() => { const s = ${status}; return ${cancel}?.disabled === true && s !== '' && !s.includes('Executing') && ${visibleTab}.querySelector('.result-loading') === null; })()`,
    timeout,
  );
  return page.eval(status);
}

/** Empties the status of the visible tab, so a wait cannot match the old one. */
export const clearStatus = (page) => page.eval(`(() => { const s = ${statusSpan}; if (s) s.textContent = ''; })()`);

/** Types `sql` as the visible editor's whole text and runs it; returns the status. */
export async function runSql(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.eval(`${visibleTab}.querySelectorAll('.notice').forEach(n => n.dataset.e2eOld = '1')`);
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(
    `(() => { const s = ${status}; const fresh = ${visibleTab}.querySelector('.notice:not([data-e2e-old])'); return ${cancel}?.disabled === true && ((s !== '' && !s.includes('Executing')) || (fresh !== null && !fresh.textContent.includes('Executing'))); })()`,
    20_000,
  );
  return page.eval(status);
}

/** The loaded rows of the visible result grid as TSV with its header (the grid draws on a
 *  canvas: it is read through Copy with headers, the clipboard stubbed). */
export async function gridText(page) {
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }; window.__copied = undefined`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("C", "KeyC", 67, CTRL | 8);
  await page.waitFor(`window.__copied !== undefined`);
  return page.eval(`window.__copied`);
}

/** Errors shown anywhere: notices, toasts, the visible tab's error. */
export const errors = `[...document.querySelectorAll('.notice.error, .toast.error, .error-toast, .result-error')].map(e => e.textContent)`;

export const shotOf = (shots, prefix) => (page, name) => shots && page.screenshot(join(shots, `${prefix}-${name}.png`));
