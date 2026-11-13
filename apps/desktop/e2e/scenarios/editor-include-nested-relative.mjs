// SQL-09. A relative `@include` inside an included file is resolved against the editor tab's
// file, not against the file that contains it (psql \ir, SQL*Plus @@): a master script that
// includes `a.sql`, which includes `b.sql` from its own folder, fails ("Save the script first,
// or give @include an absolute path.") or, when the tab is saved elsewhere, reads another b.sql.
// Steps: SQLite, run `@include <dir>/sub/a.sql`; a.sql holds `@include b.sql` and a SELECT;
// b.sql sits next to it.
// Expected: b.sql's and a.sql's statements run.
// Actual: Error on the nested @include.
// Code: apps/desktop/src/components/QueryTab.tsx command() "include": resolvePath(c.path,
// tab.filePath); planned statements of a file do not keep the file's path.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { openEditor, resultError, resultTabs, runScript, saveConnections } from "../editor-helpers.mjs";

export function setup({ dir }) {
  saveConnections(dir, [], { sqliteSql: "CREATE TABLE t (id INTEGER);" });
  mkdirSync(join(dir, "sub"));
  writeFileSync(join(dir, "sub", "a.sql"), "@include b.sql\nSELECT 'a' AS a;\n");
  writeFileSync(join(dir, "sub", "b.sql"), "SELECT 'b' AS b;\n");
}

export default async function (page, { dir }) {
  await openEditor(page, "QA SQLite");
  await runScript(page, `@include ${join(dir, "sub", "a.sql")}`);
  assert.equal(await page.eval(resultError), null);
  assert.deepEqual(await page.eval(resultTabs), ["1. Included 2 statements", "2. Included 1 statement", "3. 1 row", "4. 1 row"]);
}
