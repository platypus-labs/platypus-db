// Safety: a DELETE without WHERE asks first with the number of rows; on a production
// connection every change asks first, the data editor shows its SQL before saving, and an
// open transaction shows its age and warns when it stays open too long.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const modalButton = (text) => `[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

function seed(path) {
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c');");
  db.close();
}

export function setup({ dir }) {
  seed(join(dir, "dev.db"));
  seed(join(dir, "prod.db"));
  const connection = (id, name, file, type) => `
[[connections]]
id = "00000000-0000-4000-8000-00000000000${id}"
name = "${name}"
driver = "sqlite"
database = '${join(dir, file)}'
save_password = false
${type ? `connection_type = "${type}"` : ""}
`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1\n${connection(6, "Dev db", "dev.db", null)}${connection(7, "Prod db", "prod.db", "prod")}`,
  );
  // About a second, so the warning shows without waiting minutes.
  writeFileSync(join(dir, "settings.toml"), "[safety]\ntransaction_warning_minutes = 0.02\n");
}

async function openEditor(page, name) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`document.querySelectorAll('#e2e-conn').forEach(e => e.removeAttribute('id')); ${treeRow(name)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
}

async function runSql(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.key("Enter", "Enter", 13, CTRL);
}

export default async function (page, { dir }) {
  // Without WHERE: asked first, with the rows it would change.
  await openEditor(page, "Dev db");
  await runSql(page, "DELETE FROM t");
  await page.waitFor(`document.querySelector('.modal.guard') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal.guard').textContent`), /every row of t: about 3 rows/);
  await page.eval(`${modalButton("Cancel")}.click()`);
  await page.waitFor(`document.querySelector('.modal.guard') === null`);
  const dev = new DatabaseSync(join(dir, "dev.db"));
  assert.equal(dev.prepare("SELECT count(*) AS n FROM t").get().n, 3);
  dev.close();

  // Production: every change is listed first, then runs in the open transaction.
  await openEditor(page, "Prod db");
  await page.waitFor(`${visibleTab}.querySelector('.conn-chip.prod') !== null`);
  await runSql(page, "UPDATE t SET v = 'z' WHERE id = 1");
  await page.waitFor(`document.querySelector('.modal.guard') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal.guard h2').textContent`), /Prod db \(production\)/);
  assert.match(await page.eval(`document.querySelector('.guard-statements').textContent`), /UPDATE t SET v = 'z' WHERE id = 1/);
  await page.eval(`${modalButton("Run")}.click()`);
  await page.waitFor(`${status}.includes('affected')`, 15_000);
  await page.waitFor(`/Transaction open \\d+:\\d\\d/.test(${visibleTab}.querySelector('.tx-open')?.textContent ?? '')`);
  await page.waitFor(`${visibleTab}.querySelector('.tx-open.long') !== null`, 10_000);

  // The data editor shows its SQL before saving on production.
  await runSql(page, "SELECT * FROM t");
  await page.waitFor(`${status} === '3 rows'`, 15_000);
  // Keys before the grid knows it is editable are ignored: "+ Row" turns on when it is.
  await page.waitFor(`${visibleTab}?.querySelector('.edit-bar:not([hidden]) button')?.disabled === false`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("ArrowRight", "ArrowRight", 39);
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.type("q");
  await page.key("Enter", "Enter", 13);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("s", "KeyS", 83, CTRL);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal').textContent`), /production connection/);
}
