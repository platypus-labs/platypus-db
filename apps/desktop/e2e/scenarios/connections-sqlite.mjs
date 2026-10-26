// Connectivity: connections imported from DBeaver keep their type; a production connection
// shows its badge and opens in manual commit; the connection dialog has a Network tab for the
// SSH tunnel and shared network profiles, an Init tab, and passwords from a command.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
// By its words or, for an icon button, its accessible name.
const button = (scope, text) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(text)}) || b.getAttribute('aria-label') === ${JSON.stringify(text)})`;

/** Sets a React-controlled field and tells React, as typing would. */
const setField = (selector, value) => fill(`document.querySelector(${JSON.stringify(selector)})`, value);
/** The same for the field labelled `label` inside `scope`. */
const setLabeled = (scope, label, value) =>
  fill(
    `[...document.querySelectorAll(${JSON.stringify(`${scope} label.field`)})].find(l => l.querySelector('span')?.textContent === ${JSON.stringify(label)}).querySelector('input, select, textarea')`,
    value,
  );
const fill = (element, value) => `(() => {
  const el = ${element};
  const proto = Object.getPrototypeOf(el);
  Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event(el.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
})()`;

export function setup({ dir }) {
  const db = join(dir, "prod.db");
  const sqlite = new DatabaseSync(db);
  sqlite.exec("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);");
  sqlite.close();
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
  // Idle sessions are checked every 2 seconds instead of every minute.
  writeFileSync(join(dir, "settings.toml"), "[connections]\nkeepalive_seconds = 2\n");
  // DBeaver's file where it keeps it on Windows, under the APPDATA the app is given.
  const dbeaver = join(dir, "appdata", "DBeaverData", "workspace6", "General", ".dbeaver");
  mkdirSync(dbeaver, { recursive: true });
  writeFileSync(
    join(dbeaver, "data-sources.json"),
    JSON.stringify({
      connections: {
        "sqlite-1": {
          provider: "generic",
          driver: "sqlite_jdbc",
          name: "Prod copy",
          configuration: { url: `jdbc:sqlite:${db}`, type: "prod" },
        },
        "postgres-jdbc-1": {
          provider: "postgresql",
          driver: "postgres-jdbc",
          name: "Remote PG",
          configuration: {
            host: "db.example.com",
            port: "5432",
            database: "app",
            user: "me",
            type: "dev",
            handlers: { ssh_tunnel: { enabled: true, properties: { host: "bastion", port: "22", user: "ops", authType: "PASSWORD" } } },
          },
        },
      },
    }),
  );
}

export const env = ({ dir }) => ({ APPDATA: join(dir, "appdata") });

// No saved connections: the navigator only offers the import.
export const ready = `document.querySelector('.empty-import') !== null`;

export default async function (page, { dir, shots }) {
  await page.waitFor(`document.querySelector('.empty-import') !== null`);
  await page.eval(`document.querySelector('.empty-import').click()`);
  await page.waitFor(`document.querySelector('.import-tile.found') !== null`);
  await page.eval(`document.querySelector('.import-tile.found').click()`);
  await page.waitFor(`document.querySelectorAll('.import-row.connection').length === 2`);
  await page.eval(`${button("document.querySelector('.modal')", "Import 2")}.click()`);
  await page.waitFor(`${treeRow("Prod copy")} !== undefined && ${treeRow("Remote PG")} !== undefined`);

  const saved = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.match(saved, /connection_type = "prod"/);
  assert.match(saved, /\[connections\.ssh\]\s+host = "bastion"/);
  assert.equal(await page.eval(`${treeRow("Prod copy")}.querySelector('.type-badge')?.textContent`), "PROD");

  // Production starts in manual commit.
  await page.eval(`${treeRow("Prod copy")}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("SELECT * FROM t");
  await page.key("Enter", "Enter", 13, 2);
  await page.waitFor(`${visibleTab}.querySelector('.statusbar span')?.textContent === '1 row'`, 15_000);
  await page.waitFor(`${button(visibleTab, "Manual commit")} !== undefined`);
  // An idle session is checked: its latency shows in the status bar.
  await page.waitFor(`(${visibleTab}.querySelector('.statusbar .health.ok')?.textContent ?? '').endsWith(' ms')`, 15_000);

  // The dialog of the imported connection shows its SSH server.
  await page.eval(`[...${treeRow("Remote PG")}.querySelectorAll('button')].find(b => b.title === 'Edit').click()`);
  // SSH and SSL are on the Network tab.
  await page.waitFor(`document.querySelector('.dialog-tabs') !== null`);
  assert.equal(await page.eval(`document.querySelector('[data-page="network"]').hidden`), true);
  // The color swatches stay round dots, not stretched to a button's height.
  assert.equal(await page.eval(`(() => { const r = document.querySelector('.modal .swatch').getBoundingClientRect(); return r.width === r.height; })()`), true);
  const tab = (name) => page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('${name}')).click()`);
  await tab("Network");
  await page.waitFor(`document.querySelector('[data-page="network"]').hidden === false`);
  assert.equal(await page.eval(`document.querySelector('.ssh-section input').value`), "bastion");
  if (shots) await page.screenshot(join(shots, "connection-ssh.png"));

  // A new network profile is chosen for the connection in place of its own tunnel.
  await page.eval(`${button("document.querySelector('[data-page=\"network\"]')", "New")}.click()`);
  await page.waitFor(`document.querySelector('.network-profile') !== null`);
  await page.eval(setField(".network-profile input", "Office VPN"));
  await page.eval(`document.querySelector('.network-profile .check input').click()`);
  await page.waitFor(`document.querySelector('.network-profile .ssh-section input') !== null`);
  await page.eval(setLabeled(".network-profile", "SSH host", "jump.example.com"));
  await page.eval(setLabeled(".network-profile", "SSH user", "ops"));
  await page.eval(`${button("document.querySelector('.network-profile')", "Save")}.click()`);
  await page.waitFor(`document.querySelector('.network-profile') === null`);
  await page.waitFor(`document.querySelector('[data-field="profile_summary"]')?.textContent.includes('jump.example.com')`);
  if (shots) await page.screenshot(join(shots, "connection-profile.png"));

  // Session setup, and the password printed by a command.
  await tab("Init");
  await page.eval(setField('[data-field="init_schema"]', "app"));
  await page.eval(setField('[data-field="init_statements"]', "SET statement_timeout = '5s';"));
  if (shots) await page.screenshot(join(shots, "connection-init.png"));
  await tab("General");
  await page.eval(setField('[data-field="password_source"]', "command"));
  await page.waitFor(`document.querySelector('[data-field="password_command"]') !== null`);
  await page.eval(setField('[data-field="password_command"]', "echo s3cret"));
  await page.eval(`${button("document.querySelector('.modal')", "Save")}.click()`);
  await page.waitFor(`document.querySelector('.modal') === null`);

  const after = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.match(after, /^version = 7/m);
  assert.match(after, /\[\[network_profiles\]\][^[]*name = "Office VPN"/);
  assert.match(after, /\[network_profiles\.ssh\]\s+host = "jump.example.com"/);
  assert.match(after, /password_command = "echo s3cret"/);
  assert.match(after, /network_profile = "[0-9a-f-]{36}"/);
  assert.match(after, /\[connections\.init\]\s+schema = "app"/);
  // The profile's tunnel replaced the connection's own.
  assert.doesNotMatch(after, /host = "bastion"/);

  await connectionUrls(page, shots);
}

/** Pastes `text` into the field `selector`, as Ctrl+V would. */
const paste = (selector, text) => `(() => {
  const data = new DataTransfer();
  data.setData('text/plain', ${JSON.stringify(text)});
  document.querySelector(${JSON.stringify(selector)}).dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
})()`;

/** The input or select of the connection form's field labelled `label`. */
const formField = (label) =>
  `[...document.querySelectorAll('.connection-dialog label.field')].find(l => l.querySelector('span')?.textContent === ${JSON.stringify(label)})?.querySelector('input, select')`;

const picker = `document.querySelector('.modal.driver-picker')`;
const urlField = '.driver-picker [data-field="connection_url"]';
const headerIcon = `document.querySelector('.connection-dialog .dialog-header .driver-icon')`;

// A URL pasted in the picker opens the form filled in; the form of a new connection has no
// driver to change, only the database's logo, and Back returns to the picker. A URL for
// another database pasted in the form offers to go back; a connection's menu copies its URL
// without the password.
async function connectionUrls(page, shots) {
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  await page.waitFor(`document.querySelector(${JSON.stringify(urlField)}) !== null`);
  await page.eval(paste(urlField, "postgres://u:p@db.example:6543/shop?sslmode=require"));
  await page.waitFor(`${picker} === null && document.querySelector('.connection-dialog') !== null`);
  const value = (label) => page.eval(`${formField(label)}?.value`);
  assert.equal(await value("Name"), "shop");
  assert.equal(await value("Host"), "db.example");
  assert.equal(await value("Port"), "6543");
  assert.equal(await value("Database"), "shop");
  assert.equal(await value("User"), "u");
  assert.equal(await value("Password"), "p");
  assert.equal(await value("SSL mode"), "require");
  assert.equal(await page.eval(`${formField("Driver")} === undefined`), true);
  assert.equal(await page.eval(`${headerIcon}?.getAttribute('aria-label')`), "PostgreSQL");
  if (shots) await page.screenshot(join(shots, "connection-from-url.png"));

  // Back: the picker again, with nothing of the form kept.
  await page.eval(`${button("document.querySelector('.connection-dialog .actions')", "Back")}.click()`);
  await page.waitFor(`${picker} !== null && document.querySelector('.connection-dialog') === null`);
  assert.equal(await page.eval(`document.querySelector(${JSON.stringify(urlField)}).value`), "");

  // What is not a connection URL says so under the field.
  await page.eval(setField(urlField, "not a url"));
  await page.eval(`document.querySelector(${JSON.stringify(urlField)}).focus()`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.driver-url [role="alert"]') !== null`);
  assert.equal(await page.eval(`${picker} !== null`), true);

  // MySQL's form, with a PostgreSQL URL pasted in the host field: it stays MySQL's.
  await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === 'MySQL').click()`);
  await page.waitFor(`${headerIcon}?.getAttribute('aria-label') === 'MySQL'`);
  await page.eval(`(${formField("Host")}).id = 'e2e-host'`);
  await page.eval(paste("#e2e-host", "postgres://u@db.example/shop"));
  await page.waitFor(`document.querySelector('.connection-dialog .url-other') !== null`);
  assert.equal(await value("Host"), "localhost");
  assert.equal(await page.eval(`${headerIcon}.getAttribute('aria-label')`), "MySQL");
  // Its Back takes the URL to the picker's field.
  await page.eval(`${button("document.querySelector('.url-other')", "Back")}.click()`);
  await page.waitFor(`${picker} !== null`);
  assert.equal(await page.eval(`document.querySelector(${JSON.stringify(urlField)}).value`), "postgres://u@db.example/shop");
  await page.eval(`${button(picker, "Cancel")}.click()`);
  await page.waitFor(`document.querySelector('.modal') === null`);

  // Copy as URL: no password, which the connection does not keep in its file either.
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`${treeRow("Remote PG")}.id = 'e2e-remote'`);
  await page.clickSel("#e2e-remote", { button: "right" });
  const copyItem = `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim().startsWith('Copy as URL'))`;
  await page.waitFor(`${copyItem} !== undefined`);
  await page.eval(`${copyItem}.click()`);
  await page.waitFor(`window.__copied !== undefined`);
  assert.match(await page.eval(`window.__copied`), /^postgres:\/\/me@db\.example\.com:5432\/app(\?sslmode=[a-z-]+)?$/);
}
