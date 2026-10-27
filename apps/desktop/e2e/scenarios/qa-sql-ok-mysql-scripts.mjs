// Regression guard (passes): MySQL scripts. DELIMITER with `//` and `;;` (on lines of their
// own), `delimiter $` lower-case with a spaced `$`, strings with backslash escapes, `#`
// comments, user variables across statements, `?` parameters but not inside strings.
import assert from "node:assert/strict";
import { DB, answerParams, clearStatus, gridTsv, openEditor, resultError, resultTabs, runScript, saveConnections, setEditor, skipUnless, waitDone } from "../qa-sql.mjs";
import { CTRL } from "../cdp.mjs";

export const skip = () => skipUnless("mysql");
export const setup = ({ dir }) => saveConnections(dir, ["mysql"]);

async function script(page, sql, tabs, last) {
  await runScript(page, sql);
  assert.equal(await page.eval(resultError), null, sql);
  assert.equal((await page.eval(resultTabs)).length, tabs, sql);
  assert.equal(await gridTsv(page), last, sql);
}

export default async function (page) {
  await openEditor(page, DB.mysql.name);
  await script(
    page,
    "DROP PROCEDURE IF EXISTS qa_sql_ok_p;\nDELIMITER //\nCREATE PROCEDURE qa_sql_ok_p()\nBEGIN\n  SELECT 1 AS one;\nEND//\nDELIMITER ;\nCALL qa_sql_ok_p();",
    3,
    "one\n1",
  );
  await script(
    page,
    "DROP FUNCTION IF EXISTS qa_sql_ok_fn;\nDELIMITER ;;\nCREATE FUNCTION qa_sql_ok_fn(a INT) RETURNS INT DETERMINISTIC\nBEGIN\n  RETURN a + 1;\nEND;;\nDELIMITER ;\nSELECT qa_sql_ok_fn(1) AS r;",
    3,
    "r\n2",
  );
  await script(page, "delimiter $\nSELECT 1 AS one $\ndelimiter ;\nSELECT 2 AS two;", 2, "two\n2");
  await script(page, "SELECT 'it\\'s; ok' AS s;\nSELECT 2 AS two;", 2, "two\n2");
  await script(page, "SELECT 1 AS a; # comment ; here\nSELECT 2 AS b;", 2, "b\n2");
  await script(page, "SET @qa := 10;\nSELECT @qa * 2 AS d;", 2, "d\n20");
  await setEditor(page, "SELECT ? AS a, '?' AS b");
  await clearStatus(page);
  await page.key("Enter", "Enter", 13, CTRL);
  await answerParams(page, ["x"]);
  assert.equal(await waitDone(page), "1 row");
  assert.equal(await gridTsv(page), "a\tb\nx\t?");
  await runScript(page, "DROP PROCEDURE qa_sql_ok_p;\nDROP FUNCTION qa_sql_ok_fn;");
}
