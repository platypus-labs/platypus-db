// Regression guard (passes): PostgreSQL scripts split where they should. Dollar-quoted
// PL/pgSQL bodies with `;` and comments inside, DO blocks, E'' strings with \', nested block
// comments, BEGIN ATOMIC bodies; `::` casts, array slices, dollar strings and the jsonb `?`
// operator are not taken for parameters.
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, resultError, resultTabs, runScript, runStatement, saveConnections, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

async function script(page, sql, tabs, last) {
  await runScript(page, sql);
  assert.equal(await page.eval(resultError), null, sql);
  assert.equal((await page.eval(resultTabs)).length, tabs, sql);
  assert.equal(await gridTsv(page), last, sql);
}

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await script(
    page,
    "CREATE OR REPLACE FUNCTION qa_sql_ok_f(a int) RETURNS int LANGUAGE plpgsql AS $body$\nDECLARE x int := 1;\nBEGIN\n  IF a > 0 THEN x := a; END IF;\n  RETURN x + 1; -- ; comment\nEND;\n$body$;\nSELECT qa_sql_ok_f(41) AS r;",
    2,
    "r\n42",
  );
  await script(page, "DO $$ BEGIN RAISE NOTICE 'hi;'; END $$;\nSELECT 2 AS two;", 2, "two\n2");
  await script(page, "SELECT E'it\\'s;' AS s;\nSELECT 3 AS three;", 2, "three\n3");
  await script(page, "/* outer /* inner ; */ still ; */ SELECT 4 AS four;\nSELECT 5 AS five;", 2, "five\n5");
  await script(
    page,
    "CREATE OR REPLACE FUNCTION qa_sql_ok_g(a int) RETURNS int LANGUAGE sql BEGIN ATOMIC SELECT a + 1; END;\nSELECT qa_sql_ok_g(1) AS g;",
    2,
    "g\n2",
  );
  // No parameters dialog for these: they run straight away.
  assert.equal(await runStatement(page, "SELECT ':no'::text AS s, (ARRAY[1,2,3])[1:2] AS sl, $$ :nope $$ AS d, '{\"a\":1}'::jsonb ? 'a' AS has"), "1 row");
  assert.equal(await gridTsv(page), "s\tsl\td\thas\n:no\t{1,2}\t :nope \ttrue");
  await runScript(page, "DROP FUNCTION qa_sql_ok_f(int);\nDROP FUNCTION qa_sql_ok_g(int);");
}
