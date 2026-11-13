// Regression guard: deleting a connection asks first (Cancel keeps it), then closes its query
// tabs, takes it out of the navigator, of connections.toml and of its project.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { menu, menuItem, openQueryTab, run, treeRow } from "../conn-helpers.mjs";

const ID = "00000000-0000-4000-8000-0000000000d7";

export function setup({ dir }) {
  const file = join(dir, "del.db");
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE t (a)");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[projects]]
id = "00000000-0000-4000-9000-0000000000d9"
name = "QA delete"
color = "#d08f20"
connections = ["${ID}"]

[[connections]]
id = "${ID}"
name = "QA doomed"
driver = "sqlite"
database = '${file}'
save_password = false

[[connections]]
id = "00000000-0000-4000-8000-0000000000d8"
name = "QA stays"
driver = "sqlite"
database = '${file}'
save_password = false
`,
  );
}

async function askDelete(page) {
  await page.eval(`${treeRow("QA doomed")}.id = 'qa-doomed'`);
  await menu(page, "#qa-doomed");
  await menuItem(page, "Delete");
  await page.waitFor(`document.querySelector('.modal') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal').innerText`), /Delete the connection "QA doomed"\?/);
}

export default async function (page, { dir }) {
  await openQueryTab(page, "QA doomed");
  assert.equal(await run(page, "SELECT 1"), "1 row");
  await openQueryTab(page, "QA stays");

  await askDelete(page);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal') === null`);
  assert.equal(await page.eval(`${treeRow("QA doomed")} !== undefined`), true);
  assert.equal(await page.eval(`document.querySelectorAll('.tab').length`), 2);

  await askDelete(page);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Delete').click()`);
  await page.waitFor(`${treeRow("QA doomed")} === undefined`);
  await page.waitFor(`document.querySelectorAll('.tab').length === 1`);
  assert.match(await page.eval(`document.querySelector('.tab').textContent`), /^QA stays/);
  const toml = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.doesNotMatch(toml, /QA doomed/);
  assert.doesNotMatch(toml, new RegExp(ID));
  assert.match(toml, /QA stays/);
}
