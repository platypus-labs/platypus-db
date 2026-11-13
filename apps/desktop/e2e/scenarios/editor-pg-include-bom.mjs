// SQL-11. A script file saved as UTF-8 with a BOM (SSMS, older Notepad, many editors on
// Windows) keeps the BOM when read: it is sent in front of the first statement and
// PostgreSQL answers `syntax error at or near "﻿SELECT"` (SQLite happens to tolerate it).
// Steps: PostgreSQL, run as a script `@include <dir>/bom.sql` (BOM + two SELECTs).
// Expected: both statements run.
// Actual: the first fails, the second is not executed.
// Code: apps/desktop/src-tauri/src/commands/files.rs read_text_file keeps U+FEFF; the
// splitter (platypus-sql lexer) does not treat it as whitespace either.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DB, openEditor, resultError, resultTabs, runScript, saveConnections, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  writeFileSync(join(dir, "bom.sql"), Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from("SELECT 41 + 1 AS answer;\nSELECT 2 AS two;\n")]));
}

export default async function (page, { dir }) {
  await openEditor(page, DB.pg.name);
  await runScript(page, `@include ${join(dir, "bom.sql")}`);
  assert.equal(await page.eval(resultError), null);
  assert.deepEqual(await page.eval(resultTabs), ["1. Included 2 statements", "2. 1 row", "3. 1 row"]);
}
