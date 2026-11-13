// SQL-08. Script files are read as plain UTF-8 only (see also editor-pg-include-bom.mjs):
// - a UTF-16 file (SSMS "Unicode") or a Windows-1252 one cannot be read at all: "stream did not
//   contain valid UTF-8".
// The same reader serves @include, Open script (Ctrl+O) and the saved scripts.
// Steps: SQLite, run as a script `@include <file>` for each file.
// Expected: each file's statements run (UTF-16 by its BOM, ANSI as Windows-1252).
// Actual: an error each time.
// Code: apps/desktop/src-tauri/src/commands/files.rs read_text_file (std::fs::read_to_string).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { gridTsv, openEditor, resultError, runScript, saveConnections } from "../editor-helpers.mjs";

export function setup({ dir }) {
  saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });
  writeFileSync(join(dir, "bom.sql"), Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from("SELECT 'bom' AS v;\n")]));
  writeFileSync(join(dir, "utf16.sql"), Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from("SELECT 'utf16' AS v;\r\n", "utf16le")]));
  writeFileSync(join(dir, "ansi.sql"), Buffer.from("SELECT 'caf\xe9' AS v;\n", "latin1"));
}

export default async function (page, { dir }) {
  await openEditor(page, "QA SQLite");
  const got = {};
  for (const file of ["bom.sql", "utf16.sql", "ansi.sql"]) {
    await runScript(page, `@include ${join(dir, file)}`);
    const error = await page.eval(resultError);
    got[file] = error ?? (await gridTsv(page));
  }
  assert.deepEqual(got, { "bom.sql": "v\nbom", "utf16.sql": "v\nutf16", "ansi.sql": "v\ncafé" });
}
