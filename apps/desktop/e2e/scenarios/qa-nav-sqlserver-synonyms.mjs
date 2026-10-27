// Bug NAV (low, missing objects): SQL Server synonyms are not listed in the navigator at all —
// a schema with `CREATE SYNONYM qa_nav_syn FOR …` shows its tables, views, routines,
// sequences and triggers but no synonym, so it cannot be found, opened or dropped from the
// tree (DBeaver lists them in a Synonyms folder). The Oracle driver has the same object kind
// in the same situation (not tested here: no Instant Client).
//
// Cause: there is no synonym object kind (`ObjectKind` in crates/platypus-core), so the SQL
// Server objects query (crates/platypus-driver-mssql) skips type 'SN' of sys.objects and the
// navigator has no Synonyms folder (FOLDERS in apps/desktop/src/components/Navigator.tsx).
//
// Steps: schema qa_nav_syn_s with table qa_nav_t and synonym qa_nav_syn; expand it.
// Expected: qa_nav_syn listed. Actual: missing.
import assert from "node:assert/strict";
import { DBS, connectTo, expand, needs, row, saveConnections } from "../qa-nav-helpers.mjs";

export const skip = needs("sqlserver");

const S = "qa_nav_syn_s";

function drop() {
  DBS.sqlserver.exec(`IF OBJECT_ID(N'${S}.qa_nav_syn') IS NOT NULL DROP SYNONYM ${S}.qa_nav_syn;
    IF OBJECT_ID(N'${S}.qa_nav_t') IS NOT NULL DROP TABLE ${S}.qa_nav_t;
    IF SCHEMA_ID(N'${S}') IS NOT NULL EXEC(N'DROP SCHEMA ${S}');`);
}

export function setup({ dir }) {
  drop();
  DBS.sqlserver.exec(`EXEC(N'CREATE SCHEMA ${S}'); CREATE TABLE ${S}.qa_nav_t (id int); CREATE SYNONYM ${S}.qa_nav_syn FOR ${S}.qa_nav_t;`);
  saveConnections(dir, DBS.sqlserver.connection("QA MS", "00000000-0000-4000-8000-0000000a0032"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA MS", DBS.sqlserver.password);
    await page.waitFor(`${row(S)} !== undefined`, 30_000);
    await expand(page, S, "qa_nav_t");
    await page.sleep(500);
    // Open every folder of the schema.
    await page.eval(`[...document.querySelectorAll('.tree-row[aria-expanded=false]')].forEach(r => r.querySelector('.tree-label')?.textContent?.match(/^[A-Z][a-z]+s$/) && r.querySelector('.twisty').click())`);
    await page.sleep(500);
    assert.ok(await page.eval(`${row("qa_nav_syn")} !== undefined`), "the synonym is listed");
  } finally {
    drop();
  }
}
