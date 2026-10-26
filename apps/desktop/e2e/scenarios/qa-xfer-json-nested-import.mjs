// XFER-13 (medium). Arrays and objects of a JSON file are imported as DuckDB's display text.
// Steps: a JSON file [{"id":1,"tags":["a, b","c"],"addr":{"city":"Rome"}}]; navigator ›
// main › "Import data into a new table…", Import.
// Expected: tags holds JSON text ["a, b","c"] and addr {"city":"Rome"} (the target type is
// TEXT, so the value should stay JSON that can be parsed back).
// Actual: tags holds `[a, b, c]` — the two strings "a, b" and "c" can no longer be told from
// three — and addr holds `{city: Rome}`, neither of them JSON.
// Suspected: crates/platypus-transfer/src/import.rs:437-454 (rows_text formats LIST and
// STRUCT values with Arrow's display, not as JSON).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { chooseFile, closeModal, expand, importDialog, menu, runJob, sqliteBlock } from "../qa-xfer.mjs";

const NAME = "QA SQLite";

export function setup({ dir }) {
  new DatabaseSync(join(dir, "t.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${sqliteBlock(NAME, join(dir, "t.db"))}`);
  writeFileSync(join(dir, "nested.json"), '[{"id":1,"tags":["a, b","c"],"addr":{"city":"Rome"}}]');
}

export default async function (page, { dir }) {
  await expand(page, NAME, "main");
  await menu(page, "main", "Import data into a new table", importDialog, NAME);
  assert.equal(await chooseFile(page, join(dir, "nested.json")), null);
  assert.match(await runJob(page, importDialog, "Import"), /Imported 1 row/);
  await closeModal(page, importDialog);
  const db = new DatabaseSync(join(dir, "t.db"));
  const { tags, addr } = db.prepare("SELECT tags, addr FROM nested").get();
  db.close();
  assert.deepEqual({ tags: JSON.parse(tags), addr: JSON.parse(addr) }, { tags: ["a, b", "c"], addr: { city: "Rome" } }, `stored ${tags} / ${addr}`);
}
