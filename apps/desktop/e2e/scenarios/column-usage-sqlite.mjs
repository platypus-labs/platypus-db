// Where a column is used (group Q): the "Used by" section of a table's tab lists, for each
// column, the views and triggers of the schema and the saved scripts that name it, with the
// line; a comment or a string that mentions a column does not count, nor a script linked to
// another connection; the columns named nowhere are listed apart. A row opens the object's tab
// or the script; Refresh reads again after a view is dropped.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "Usage db";
const ID = "00000000-0000-4000-8000-0000000a5e11";
const OTHER = "00000000-0000-4000-8000-0000000a5e12";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;
const usageRows = `[...(${objectTab}?.querySelectorAll('.object-body tbody tr') ?? [])].map(tr => [...tr.querySelectorAll('td')].map(td => td.textContent))`;
const unused = `(${objectTab}?.querySelector('[data-field="unused"]')?.textContent ?? '')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "usage.db"));
  db.exec(`CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT, nickname TEXT, last_login TEXT);
           CREATE TABLE audit (id INTEGER PRIMARY KEY, what TEXT);
           CREATE VIEW active_users AS
             SELECT id,
                    email
             FROM users WHERE last_login IS NOT NULL;
           -- Mentions nickname only in a comment and a string: no use.
           CREATE VIEW labels AS SELECT 'nickname' AS label /* nickname */ FROM users;
           CREATE TRIGGER users_audit AFTER UPDATE OF email ON users
           BEGIN INSERT INTO audit (what) VALUES ('email changed'); END;`);
  db.close();
  const scripts = join(dir, "scripts", "all");
  mkdirSync(scripts, { recursive: true });
  writeFileSync(join(scripts, "report.sql"), "-- Weekly report\nSELECT count(*)\nFROM users\nWHERE nickname LIKE 'a%';\n");
  writeFileSync(join(scripts, "elsewhere.sql"), "SELECT id FROM users WHERE id = 1;\n");
  writeFileSync(join(scripts, ".links.json"), JSON.stringify({ "report.sql": ID, "elsewhere.sql": OTHER }));
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "${ID}"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "usage.db")}'
save_password = false

[[connections]]
id = "${OTHER}"
name = "Other db"
driver = "sqlite"
database = '${join(dir, "usage.db")}'
save_password = false
`,
  );
}

async function openUsage(page) {
  await page.eval(`${row("users")}.querySelector('.tree-label').id = 'e2e-row'`);
  const r = await page.rect("#e2e-row");
  await page.eval(`document.getElementById('e2e-row').removeAttribute('id')`);
  await page.click(r.x + 5, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Open details')).click()`);
  await page.waitFor(`${objectTab} !== undefined`, 10_000);
  await page.eval(`[...${objectTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith('Used by')).click()`);
}

export default async function (page, { dir }) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("users")} !== undefined`, 15_000);
  await openUsage(page);
  await page.waitFor(`${usageRows}.length > 0`, 20_000);

  const rows = await page.eval(usageRows);
  const has = (column, by, kind, line) =>
    rows.some(([c, b, k, l]) => c === column && b.startsWith(by) && k === kind && l.startsWith(`${line}:`));
  assert.ok(has("id", "active_users", "View", 2), JSON.stringify(rows));
  assert.ok(has("email", "active_users", "View", 3), JSON.stringify(rows));
  assert.ok(has("last_login", "active_users", "View", 4), JSON.stringify(rows));
  assert.ok(has("email", "users_audit", "Trigger", 1), JSON.stringify(rows));
  assert.ok(has("nickname", "report.sql", "Saved script", 4), JSON.stringify(rows));
  // A comment and a string are not uses; a script on another connection is not searched.
  assert.ok(!rows.some(([, b]) => b.startsWith("labels")), JSON.stringify(rows));
  assert.ok(!rows.some(([, b]) => b.startsWith("elsewhere.sql")), JSON.stringify(rows));
  assert.equal(await page.eval(unused), "", "every column is named somewhere");

  // A row opens the view's tab; the script opens in an editor.
  await page.eval(`[...${objectTab}.querySelectorAll('.object-body tbody button.link')].find(b => b.textContent === 'active_users').click()`);
  await page.waitFor(`${objectTab}?.querySelector('.object-title strong')?.textContent === 'active_users'`, 10_000);
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith('users')).click()`);
  await page.waitFor(`${objectTab}?.querySelector('.object-title strong')?.textContent === 'users'`, 10_000);
  await page.eval(`[...${objectTab}.querySelectorAll('.object-body tbody button.link')].find(b => b.textContent === 'report.sql').click()`);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('report.sql')`, 10_000);

  // The view dropped outside the app, Refresh: last_login is named nowhere now.
  const db = new DatabaseSync(join(dir, "usage.db"));
  db.exec("DROP VIEW active_users");
  db.close();
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith('users')).click()`);
  await page.waitFor(`${objectTab}?.querySelector('.object-title strong')?.textContent === 'users'`, 10_000);
  await page.eval(`${objectTab}.querySelector('button[aria-label="Refresh"]').click()`);
  await page.waitFor(`${unused}.endsWith('last_login')`, 20_000);
  const after = await page.eval(usageRows);
  assert.ok(!after.some(([, b]) => b.startsWith("active_users")), JSON.stringify(after));
  assert.ok(after.some(([c, b]) => c === "email" && b.startsWith("users_audit")), JSON.stringify(after));
}
