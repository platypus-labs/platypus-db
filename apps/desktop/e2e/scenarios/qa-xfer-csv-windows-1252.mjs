// XFER-22 (medium). A CSV that is not UTF-8 cannot be imported at all.
// Steps: a CSV saved by Excel as "CSV (delimitato dal separatore di elenco)", i.e. Windows-1252
// (`id,city / 1,città`); navigator › main › "Import data into a new table…", choose it.
// Expected: the preview shows città — the file's encoding detected, or chosen in the dialog
// (DBeaver has an Encoding field).
// Actual: a 30-line DuckDB error ("Invalid unicode (byte sequence mismatch) detected… Set the
// correct encoding… LINE 1: CREATE OR REPLACE VIEW…") and no way to pick an encoding. UTF-16
// files ("Unicode text" from Excel) fail the same way.
// Suspected: crates/platypus-driver-duckdb/src/files.rs:56-60 (read_csv without an encoding)
// and apps/desktop/src/transfer/ImportDataDialog.tsx:335-360 (no encoding option).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { chooseFile, closeModal, expand, importDialog, menu, sqliteBlock } from "../qa-xfer.mjs";

const NAME = "QA SQLite";

export function setup({ dir }) {
  new DatabaseSync(join(dir, "t.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock(NAME, join(dir, "t.db"))}`);
  writeFileSync(join(dir, "cities.csv"), Buffer.from("id,city\n1,citt\xe0\n2,Mon\xe9\n", "latin1"));
}

export default async function (page, { dir, shots }) {
  await expand(page, NAME, "main");
  await menu(page, "main", "Import data into a new table", importDialog, NAME);
  const error = await chooseFile(page, join(dir, "cities.csv"));
  if (shots) await page.screenshot(join(shots, "qa-xfer-csv-windows-1252.png"));
  assert.equal(error, null);
  const cells = await page.eval(`[...${importDialog}.querySelectorAll('.import-preview td')].map(c => c.textContent)`);
  assert.ok(cells.includes("città"), `${cells}`);
  await closeModal(page, importDialog);
}
