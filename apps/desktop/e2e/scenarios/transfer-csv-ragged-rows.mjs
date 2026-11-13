// XFER-03 (critical). A CSV with irregular lines loses its header and the rows before them.
// Steps: a CSV "a,b / 1,2 / 3 / 4,5,6" (one short and one long line); navigator › main ›
// "Import data into a new table…", choose the file, Import.
// Expected: the header a,b is kept and every line is imported (padded or truncated), or the
// import is refused with the line that does not fit.
// Actual: DuckDB's sniffer skips everything before the last irregular line: the preview has
// columns column0..column2 and the only row 4,5,6; "Imported 1 row" — the header and two of
// the three data lines are gone, silently.
// Suspected: crates/platypus-driver-duckdb/src/files.rs:56-60 (read_csv with full
// auto-detection, including skip_rows, and no null_padding).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { chooseFile, closeModal, expand, importDialog, menu, runJob, sqliteBlock } from "../transfer-helpers.mjs";

const NAME = "QA SQLite";

export function setup({ dir }) {
  new DatabaseSync(join(dir, "t.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock(NAME, join(dir, "t.db"))}`);
  writeFileSync(join(dir, "ragged.csv"), "a,b\n1,2\n3\n4,5,6\n");
}

export default async function (page, { dir }) {
  await expand(page, NAME, "main");
  await menu(page, "main", "Import data into a new table", importDialog, NAME);
  const error = await chooseFile(page, join(dir, "ragged.csv"));
  if (error) return; // A refusal that names the problem is fine.
  const columns = await page.eval(`[...${importDialog}.querySelectorAll('.import-preview th')].map(t => t.textContent.split(' ')[0])`);
  const out = await runJob(page, importDialog, "Import");
  await closeModal(page, importDialog);
  const db = new DatabaseSync(join(dir, "t.db"));
  const rows = db.prepare("SELECT count(*) AS n FROM ragged").get().n;
  db.close();
  assert.deepEqual(columns.slice(0, 2), ["a", "b"], `columns ${columns}, ${out}`);
  assert.equal(rows, 3, out);
}
