// The navigator's Simple view: only tables and views, with their columns.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "Simple db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "simple.db"));
  db.exec(`CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);
           CREATE INDEX t_v ON t (v);
           CREATE VIEW tv AS SELECT v FROM t;
           CREATE TRIGGER t_ins AFTER INSERT ON t BEGIN SELECT 1; END;`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000009"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "simple.db")}'
save_password = false
`,
  );
  writeFileSync(join(dir, "settings.toml"), '[navigator]\nview = "simple"\n');
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.click()`);
  await page.waitFor(`${row("t")} !== undefined && ${row("Views")} !== undefined`, 15_000);
  assert.equal(await page.eval(`${row("Triggers")} === undefined`), true, "no triggers in the Simple view");

  await page.eval(`${row("t")}.click()`);
  await page.waitFor(`${row("Columns")} !== undefined`, 15_000);
  assert.equal(await page.eval(`${row("Indexes")} === undefined`), true, "no indexes in the Simple view");
}
