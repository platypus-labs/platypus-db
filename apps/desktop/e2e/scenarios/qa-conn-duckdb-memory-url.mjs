// CONN-08 (medium). DuckDB's in-memory database does not survive its own URL: Copy as URL of
// a `:memory:` connection gives `duckdb:///:memory:`, and that URL pasted in the picker opens
// the form with the database file `/:memory:` (a file path, not the in-memory database).
// Steps: DuckDB connection on `:memory:`; Copy as URL; paste the copied URL in a new
// connection's picker.
// Expected: database `:memory:` (a URL that reads back as the same connection).
// Actual: `/:memory:`.
// Suspected: crates/platypus-core/src/config.rs `read_url` (embedded path keeps the leading
// slash unless it is a Windows drive) and config/to_url.rs.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { dialog, formField, menu, menuItem, openPicker, paste, treeRow, urlField } from "../conn-helpers.mjs";

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000000c8"
name = "QA duck mem"
driver = "duckdb"
database = ":memory:"
save_password = false
`,
  );
}

export default async function (page) {
  await page.waitFor(`${treeRow("QA duck mem")} !== undefined`);
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`${treeRow("QA duck mem")}.id = 'qa-duck'`);
  await menu(page, "#qa-duck");
  await menuItem(page, "Copy as URL");
  await page.waitFor(`window.__copied !== undefined`);
  const url = await page.eval(`window.__copied`);

  await openPicker(page);
  await page.eval(paste(`document.querySelector(${JSON.stringify(urlField)})`, url));
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await page.eval(`${formField("Database file")}.value`), ":memory:", `from ${url}`);
}
