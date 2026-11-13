// qa5-xfer-1 (medium). A JSON file that starts with a byte order mark imports as one row with one
// column "json" holding the whole array as text.
// Steps: a file `[{"id":1,"v":"bom"},{"id":2,"v":"x"}]` saved as UTF-8 with BOM (what Windows
// PowerShell's Out-File/Set-Content -Encoding utf8 and Notepad write); navigator > main > "Import
// data into a new table...", choose the file.
// Expected: the preview shows the columns id and v with two rows, as for the same file without
// the BOM (a CSV with a BOM already imports correctly).
// Actual: one column `json` (type STRUCT(id BIGINT, v VARCHAR)[]) and one row with the text of the
// whole array: DuckDB's read_json does not see the "[" as the first character.
// Code: crates/platypus-driver-duckdb/src/files.rs (the read_json call) and the file reading in
// crates/platypus-transfer/src/import.rs: strip a UTF-8 BOM (or detect it as the CSV path does)
// before the format is guessed.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { chooseFile, closeModal, expand, importDialog, menu, sqliteBlock } from "../transfer-helpers.mjs";

const NAME = "QA SQLite";

export function setup({ dir }) {
  new DatabaseSync(join(dir, "t.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock(NAME, join(dir, "t.db"))}`);
  writeFileSync(join(dir, "bom.json"), `\ufeff[{"id":1,"v":"bom"},{"id":2,"v":"x"}]`);
}

export default async function (page, { dir }) {
  await expand(page, NAME, "main");
  await menu(page, "main", "Import data into a new table", importDialog, NAME);
  assert.equal(await chooseFile(page, join(dir, "bom.json")), null);
  const heads = await page.eval(`[...${importDialog}.querySelectorAll('.import-preview th')].map(c => c.textContent.replace(/ .*/, ''))`);
  assert.deepEqual(heads, ["id", "v"], `columns: ${heads}`);
  await closeModal(page, importDialog);
}
