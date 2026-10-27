// Helpers of the qa-safe scenarios: saved connections to the docker/compose.yml databases
// (passwords from a password command, so the keychain is never touched), statements run
// beside the app with the database's own client inside its container, and the guard check.
import { spawnSync } from "node:child_process";
import { connect as tcp } from "node:net";
import { CTRL } from "./cdp.mjs";

export const DBS = {
  postgres: {
    port: 55432,
    container: "platypus-db-postgres-1",
    toml: `driver = "postgres"\nhost = "localhost"\nport = 55432\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\nsave_password = false\npassword_command = "echo platypus"\n`,
    client: (sql) => ["psql", "-U", "platypus", "-d", "platypus", "-tAc", sql],
  },
  mysql: {
    port: 53306,
    container: "platypus-db-mysql-1",
    toml: `driver = "mysql"\nhost = "127.0.0.1"\nport = 53306\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\nsave_password = false\npassword_command = "echo platypus"\n`,
    client: (sql) => ["mysql", "-uplatypus", "-pplatypus", "-N", "-B", "platypus", "-e", sql],
  },
  sqlserver: {
    port: 51433,
    container: "platypus-db-sqlserver-1",
    toml: `driver = "sqlserver"\nhost = "localhost"\nport = 51433\ndatabase = "platypus"\nuser = "sa"\nssl_mode = "disable"\nsave_password = false\npassword_command = "echo Platypus-2026"\n`,
    client: (sql) => ["/opt/mssql-tools18/bin/sqlcmd", "-C", "-S", "localhost", "-U", "sa", "-P", "Platypus-2026", "-d", "platypus", "-h", "-1", "-W", "-Q", `SET NOCOUNT ON; ${sql}`],
  },
  mongo: {
    port: 57017,
    container: "platypus-db-mongo-1",
    toml: `driver = "mongo"\nhost = "localhost"\nport = 57017\ndatabase = "platypus?authSource=admin"\nuser = "platypus"\nssl_mode = "disable"\nsave_password = false\npassword_command = "echo platypus"\n`,
    client: (js) => ["mongosh", "--quiet", "-u", "platypus", "-p", "platypus", "--authenticationDatabase", "admin", "platypus", "--eval", js],
  },
};

/** Runs `sql` with the database's client in its container; returns its output, trimmed. */
export function dbExec(kind, sql) {
  const db = DBS[kind];
  const r = spawnSync("docker", ["exec", db.container, ...db.client(sql)], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${kind}: ${sql}: ${r.stderr}`);
  return r.stdout.trim();
}

/** A skip() for scenarios that need `kind`. */
export function needs(kind) {
  const { port } = DBS[kind];
  return () =>
    new Promise((resolve) => {
      const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
      const done = (reason) => {
        socket.destroy();
        resolve(reason);
      };
      socket.on("connect", () => done(null));
      socket.on("error", () => done(`no ${kind} on port ${port} (docker compose -f docker/compose.yml up -d)`));
      socket.on("timeout", () => done(`no ${kind} on port ${port}`));
    });
}

/** A `[[connections]]` table for `kind`. */
export const connectionToml = (kind, id, name, extra = "") => `
[[connections]]
id = "${id}"
name = "${name}"
${DBS[kind].toml}${extra}
`;

export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
export const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
export const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;

/** Opens an editor on the connection `name`. */
export async function openEditor(page, name) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`document.querySelectorAll('#e2e-conn').forEach(e => e.removeAttribute('id')); ${treeRow(name)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 20_000);
}

/** The read-only notice of the visible tab, or ''. */
export const readOnlyNotice = `(${visibleTab}?.querySelector('.notice.warn')?.textContent ?? '')`;

/** Types `sql` and runs it; returns "guard: …" when the confirmation opened (and cancels it),
 *  "read-only: …" when the tab's read-only notice stopped it (`readOnly`: on a read-only
 *  connection, in a tab that showed no notice yet), else the status the run ended with. */
export async function runOrGuard(page, sql, { readOnly = false } = {}) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  // The completion list may open on the typed names (once the catalog is loaded) and take the
  // key: close it first.
  await page.sleep(150);
  if (await page.eval(`document.querySelector('.cm-tooltip-autocomplete') !== null`)) {
    await page.key("Escape", "Escape", 27);
    await page.waitFor(`document.querySelector('.cm-tooltip-autocomplete') === null`);
  }
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(
    `document.querySelector('.modal.guard') !== null || ${readOnly} && /read-only/.test(${readOnlyNotice}) || (() => { const s = ${status}; return ${cancel}?.disabled === true && s !== '' && !s.includes('Executing'); })()`,
    60_000,
  );
  // The guard estimates rows on the tab's session, which can wait behind the catalog load
  // for completion right after connecting (seen on SQL Server): hence the long timeout above.
  if (readOnly && /read-only/.test(await page.eval(readOnlyNotice))) return `read-only: ${await page.eval(readOnlyNotice)}`;
  if (await page.eval(`document.querySelector('.modal.guard') !== null`)) {
    const text = await page.eval(`document.querySelector('.modal.guard').textContent`);
    await page.eval(`[...document.querySelectorAll('.modal.guard button')].find(b => ['Cancel', 'Close'].includes(b.textContent.trim())).click()`);
    await page.waitFor(`document.querySelector('.modal.guard') === null`);
    return `guard: ${text}`;
  }
  return page.eval(status);
}
