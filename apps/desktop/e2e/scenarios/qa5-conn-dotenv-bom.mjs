// QA5-CONN-1 (medium). A `.env` file saved with a UTF-8 byte-order mark (Windows Notepad's
// "UTF-8 with BOM", PowerShell 5's `Out-File`/`>`) loses its first variable: the connection
// that reads it says "the environment variable ... is not set" though the line is there.
// Steps: project.env = BOM + `QA5_DB='<path>'` (CRLF lines); a SQLite connection whose
//   database is `${env:QA5_DB}` and whose "Variables file" is that file; expand it.
// Expected: the connection opens (the BOM is not part of the first name).
// Actual: "the environment variable QA5_DB is not set".
// Code: crates/platypus-core/src/env.rs `parse_dotenv` / `EnvVars::load`: `read_to_string`
//   keeps U+FEFF, so the first name is "\u{feff}QA5_DB", which is not alphanumeric and is skipped.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { row, saveConnections } from "../qa-nav-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "bom.db"));
  db.exec("CREATE TABLE from_bom (id INTEGER PRIMARY KEY)");
  db.close();
  writeFileSync(join(dir, "project.env"), `\uFEFFQA5_DB='${join(dir, "bom.db")}'\r\nOTHER=1\r\n`);
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a5101"
name = "QA5 bom"
driver = "sqlite"
database = '\${env:QA5_DB}'
save_password = false

[connections.options]
env_file = '${join(dir, "project.env")}'
`);
}

export default async function (page) {
  await page.waitFor(`${row("QA5 bom")} !== undefined`);
  await page.eval(`${row("QA5 bom")}.querySelector(".twisty").click()`);
  await page.waitFor(
    `${row("from_bom")} !== undefined || [...document.querySelectorAll('.notice, .toast')].some(n => n.textContent.includes('is not set'))`,
    15_000,
  );
  const msg = await page.eval(`[...document.querySelectorAll('.notice, .toast')].map(n => n.textContent).join(' | ')`);
  assert.ok(await page.eval(`${row("from_bom")} !== undefined`), `the connection did not open: ${msg}`);
}
