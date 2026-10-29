// "Test connections" from the navigator's list menu: every connection of All connections
// connected to and closed again, with the server's version and time; a database that does
// not answer shows its error, one that asks for its password each time is not tried, and
// the summary counts them. In a project, the project's menu tests its connections only.
// Needs no server: the PostgreSQL connection points at a closed port.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const rows = `[...document.querySelectorAll('.test-connections .multi-run-row')]`;
const state = (name) => `${rows}.find(r => r.querySelector('.multi-run-name')?.textContent === ${JSON.stringify(name)})?.dataset.state`;
const outcome = (name) => `${rows}.find(r => r.querySelector('.multi-run-name')?.textContent === ${JSON.stringify(name)})?.querySelector('.multi-run-outcome')?.textContent`;
const summary = `(document.querySelector('.test-connections [data-field=summary]')?.textContent ?? '')`;
const PROJECT = "00000000-0000-4000-8000-0000000f0100";

export function setup({ dir }) {
  const file = join(dir, "ok.db");
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE t (id INTEGER)");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000f0001"
name = "Local file"
driver = "sqlite"
database = '${file}'
save_password = false

[[connections]]
id = "00000000-0000-4000-8000-0000000f0002"
name = "Closed port"
driver = "postgres"
host = "127.0.0.1"
port = 1
database = "x"
user = "x"
ssl_mode = "disable"
save_password = false
pgpass = true

[[connections]]
id = "00000000-0000-4000-8000-0000000f0003"
name = "Asks each time"
driver = "mysql"
host = "127.0.0.1"
port = 1
database = "x"
user = "x"
save_password = false

[[projects]]
id = "${PROJECT}"
name = "Only the file"
color = "#3b82f6"
connections = ["00000000-0000-4000-8000-0000000f0001"]
`,
  );
}

/** Right-clicks the empty part of the navigator and picks `item`. */
async function listMenu(page, item) {
  const r = await page.rect(".sidebar .tree");
  await page.click(r.x + 40, r.y + r.h - 20, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})).click()`);
}

export default async function (page) {
  await listMenu(page, "Test connections");
  await page.waitFor(`document.querySelector('.test-connections') !== null`);
  await page.waitFor(`${summary} !== ''`, 30_000);

  assert.equal(await page.eval(state("Local file")), "done");
  assert.match(await page.eval(outcome("Local file")), /^SQLite \S+ · \d+ ms$/);
  assert.equal(await page.eval(state("Closed port")), "failed");
  assert.ok((await page.eval(outcome("Closed port"))).length > 0, "the error shows");
  assert.equal(await page.eval(state("Asks each time")), "skipped");
  assert.equal(await page.eval(outcome("Asks each time")), "Asks for its password: not tried");
  assert.equal(await page.eval(summary), "Connected: 1 · Failed: 1 · Not tried: 1");

  // Again: the rows wait, then end the same way.
  await page.eval(`document.querySelector('.test-connections [data-action=again]').click()`);
  await page.waitFor(`${summary} === '' || document.querySelector('.test-connections [data-action=again]').disabled`);
  await page.waitFor(`${summary} === 'Connected: 1 · Failed: 1 · Not tried: 1'`, 30_000);

  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.test-connections') === null`);

  // A project's menu tests its connections only.
  const square = `[...document.querySelectorAll('.project-rail [data-project]')].find(e => e.dataset.project === ${JSON.stringify(PROJECT)})`;
  await page.waitFor(`${square} !== undefined`);
  await page.eval(`${square}.id = 'e2e-project'`);
  const r = await page.rect("#e2e-project");
  await page.click(r.x + r.w / 2, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Test connections')).click()`);
  await page.waitFor(`${summary} !== ''`, 30_000);
  assert.equal(await page.eval(`document.querySelector('.test-connections h2').textContent`), "Test the connections of Only the file");
  assert.equal(await page.eval(`${rows}.length`), 1);
  assert.equal(await page.eval(summary), "Connected: 1");
}
