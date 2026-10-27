// Regression guard: a PostgreSQL session killed on the server (pg_terminate_backend from
// another connection) is reopened by the next statement, which succeeds; a production
// session killed with a transaction open says the transaction was rolled back, then works,
// still in manual commit. Sessions are told apart by the application_name their init
// statement sets. Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { needsPort, openQueryTab, run, visibleTab } from "../conn-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

const pg = (id, name, extra) => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
password_command = "echo platypus"
${extra}`;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7
${pg("00000000-0000-4000-8000-0000000000e1", "QA pg victim", `[connections.init]\nstatements = "SET application_name = 'qa_conn_victim';"\n`)}
${pg("00000000-0000-4000-8000-0000000000e3", "QA pg prod victim", `connection_type = "prod"\n[connections.init]\nstatements = "SET application_name = 'qa_conn_prod_victim';"\n`)}
${pg("00000000-0000-4000-8000-0000000000e2", "QA pg killer", "")}`,
  );
}

const tab = (name) => `[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith(${JSON.stringify(name)}))`;
const kill = (app) => `SELECT count(pg_terminate_backend(pid)) FROM pg_stat_activity WHERE application_name = '${app}'`;

async function show(page, name) {
  await page.eval(`${tab(name)}.click()`);
  await page.waitFor(`${visibleTab}.innerText.includes(${JSON.stringify(`${name} · `)})`);
}

export default async function (page) {
  await openQueryTab(page, "QA pg victim");
  assert.equal(await run(page, "SELECT current_setting('application_name')"), "1 row");
  await openQueryTab(page, "QA pg prod victim");
  assert.equal(await run(page, "SELECT 1"), "1 row");
  // Manual commit: the SELECT opened a transaction.
  await page.waitFor(`${visibleTab}.querySelector('.toolbar').innerText.includes('Transaction open')`);

  await openQueryTab(page, "QA pg killer");
  assert.equal(await run(page, kill("qa_conn_victim")), "1 row");
  assert.equal(await run(page, kill("qa_conn_prod_victim")), "1 row");

  // Reopened on the first statement.
  await show(page, "QA pg victim");
  assert.equal(await run(page, "SELECT 2"), "1 row");

  // The open transaction is reported lost, then the session works in manual commit.
  await show(page, "QA pg prod victim");
  assert.equal(await run(page, "SELECT 2"), "Error");
  assert.match(await page.eval(`${visibleTab}.innerText`), /The connection was lost and has been reopened; the open transaction was rolled back\./);
  assert.equal(await run(page, "SELECT 3"), "1 row");
  await page.waitFor(`${visibleTab}.querySelector('.toolbar').innerText.includes('Transaction open')`);
}
