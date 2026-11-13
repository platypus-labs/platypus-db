// XFER-24 (low). `@export` to a .tsv or .csv.gz file writes plain comma-separated text.
// Steps: run as a script `@export <folder>\out.tsv` then `SELECT 1 AS a, 'x y' AS b`; again
// with `out.csv.gz`.
// Expected: out.tsv separated by tabs (the import reads .tsv as tab-separated), out.csv.gz
// gzip-compressed as the Export dialog would make it — or an error naming the extensions
// @export knows.
// Actual: both files hold `a,b\n1,x y\n`: an unknown extension silently falls back to CSV with
// commas, and the .gz file is not compressed (gzip refuses it).
// Suspected: apps/desktop/src/editor/params.ts:57-70 and 87-93 (FORMATS has no tsv/gz, and
// `?? "csv"`).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { openEditor, runScript, sqliteOnly } from "../transfer-helpers.mjs";

export const setup = ({ dir }) => sqliteOnly(dir);

export default async function (page, { dir }) {
  await openEditor(page, "QA SQLite");
  await runScript(page, `@export ${join(dir, "out.tsv")}\nSELECT 1 AS a, 'x y' AS b`);
  await runScript(page, `@export ${join(dir, "out.csv.gz")}\nSELECT 1 AS a, 'x y' AS b`);
  assert.equal(readFileSync(join(dir, "out.tsv"), "utf8").split(/\r?\n/)[0], "a\tb");
  const gz = readFileSync(join(dir, "out.csv.gz"));
  assert.deepEqual([gz[0], gz[1]], [0x1f, 0x8b], `not gzip: ${JSON.stringify(gz.toString("utf8"))}`);
}
