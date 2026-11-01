// Type-ahead in the navigator tree (typing a name moves to the row) and the `#name` filter
// that finds objects named like a SQL Server temp table when no such tag exists.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "Typeahead db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const focusedLabel = `document.activeElement?.querySelector?.('.tree-label')?.textContent ?? null`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "typeahead.db"));
  db.exec(`CREATE TABLE yak (id INTEGER); CREATE TABLE zebra (id INTEGER); CREATE TABLE zulu (id INTEGER);
           CREATE TABLE "#temp1" (id INTEGER);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-00000000000a"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "typeahead.db")}'
save_password = false
`,
  );
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${row("zebra")} !== undefined && ${row("yak")} !== undefined`, 15_000);

  // From the first table, "z" goes to the next row starting with z, case-insensitively.
  await page.eval(`${row("yak")}.focus()`);
  await page.waitFor(`${focusedLabel} === "yak"`);
  await page.type("Z");
  await page.waitFor(`${focusedLabel} === "zebra"`);

  // A prefix typed in quick succession narrows on the same word.
  await page.type("u");
  await page.type("l");
  await page.waitFor(`${focusedLabel} === "zulu"`);

  // After a pause the text starts over: "y" alone finds yak, wrapping backwards in the list.
  await new Promise((r) => setTimeout(r, 1300));
  await page.type("y");
  await page.waitFor(`${focusedLabel} === "yak"`);

  // No match: focus stays where it is.
  await new Promise((r) => setTimeout(r, 1300));
  await page.type("q");
  assert.equal(await page.eval(focusedLabel), "yak", "no row starts with q");

  // The filter box keeps its keys: typing there does not move the tree.
  await page.eval(`document.querySelector('.tree-filter').focus()`);
  await page.type("zul");
  await page.waitFor(`document.querySelector('.tree-filter').value === "zul"`);
  assert.equal(await page.eval(`document.activeElement === document.querySelector('.tree-filter')`), true);

  // `#temp` is no known tag: it is searched as part of the name.
  await page.eval(`document.querySelector('.tree-filter').select()`);
  await page.type("#temp");
  await page.waitFor(`document.querySelector('.tree-filter').value === "#temp"`);
  await page.waitFor(`${row("#temp1")} !== undefined && ${row("zebra")} === undefined`, 15_000);
}
