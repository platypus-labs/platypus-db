// DBS-23 (low): "Copy to a database" from CockroachDB to PostgreSQL turns an array column into
// TEXT: STRING[] becomes a text column holding '{x,y}', although both databases have arrays of
// the same syntax (DBeaver's transfer creates text[]). Queries on the copy (tags[1], ANY(tags),
// array_length) then fail, and the type has to be fixed by hand.
//
// Steps: CockroachDB (docker profile pgwire) qa_dbs.c (id INT8 PRIMARY KEY, tags STRING[]),
// one row; navigator › c › Copy to a database…, target the PostgreSQL connection (public),
// new table; Copy.
// Expected: public.c.tags of type text[] holding {x,y}.
// Actual: tags of type text.
// Suspected: crates/platypus-transfer (the target type of an Arrow List column, mapped to TEXT
// whatever the target dialect).
import assert from "node:assert/strict";
import { conns, needsPorts, openPath, sh } from "../drivers-helpers.mjs";
import { copyTarget, importDialog, menu, runJob, closeModal } from "../transfer-helpers.mjs";

export const skip = needsPorts(56257, 55432);

export function setup({ dir }) {
  sh("cockroach", "DROP SCHEMA IF EXISTS qa_dbs CASCADE; CREATE SCHEMA qa_dbs; CREATE TABLE qa_dbs.qa_dbs_c (id INT8 PRIMARY KEY, tags STRING[]); INSERT INTO qa_dbs.qa_dbs_c VALUES (1, ARRAY['x', 'y']);");
  sh("pg", "DROP TABLE IF EXISTS public.qa_dbs_c;");
  conns(dir, [["QA CRDB", "cockroach"], ["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA CRDB", "platypus", "qa_dbs", "Tables", "qa_dbs_c"]);
    await menu(page, "qa_dbs_c", "Copy to a database", importDialog, ["QA CRDB", "platypus", "qa_dbs"]);
    assert.equal(await copyTarget(page, "QA PG"), null);
    assert.match(await runJob(page, importDialog, "Copy", 60_000), /Imported 1 row/);
    await closeModal(page, importDialog);
    assert.equal(sh("pg", "SELECT data_type FROM information_schema.columns WHERE table_name = 'qa_dbs_c' AND column_name = 'tags'"), "ARRAY");
  } finally {
    sh("cockroach", "DROP SCHEMA IF EXISTS qa_dbs CASCADE;");
    sh("pg", "DROP TABLE IF EXISTS public.qa_dbs_c;");
  }
}
