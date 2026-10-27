// SQL-01 (fixed). MySQL: a `DELIMITER $$` script whose body ends with `END$$` (the delimiter
// glued to END, the usual style of MySQL dumps and Workbench) was not split: `$` is an
// identifier character in the MySQL lexer, so `END$$` was read as one word and the procedure,
// the `DELIMITER ;` line and the CALL went to the server as one statement.
// Steps: run as a script the text below on MySQL.
// Expected: the procedure is created and CALL returns its row, one result per statement.
// It was: "1064: You have an error in your SQL syntax ... near 'DELIMITER ;'".
// Code: crates/platypus-sql/src/lexer.rs is_ident_char ('$' in MySQL) and lib.rs split().
import assert from "node:assert/strict";
import { DB, openEditor, resultError, resultTabs, runScript, saveConnections, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("mysql");
export const setup = ({ dir }) => saveConnections(dir, ["mysql"]);

export default async function (page) {
  await openEditor(page, DB.mysql.name);
  const status = await runScript(
    page,
    "DROP PROCEDURE IF EXISTS qa_sql_glued;\nDELIMITER $$\nCREATE PROCEDURE qa_sql_glued()\nBEGIN\n  SELECT 1 AS one;\nEND$$\nDELIMITER ;\nCALL qa_sql_glued();",
  );
  const error = await page.eval(resultError);
  // The script's results, before the clean-up below replaces them with its own.
  const tabs = await page.eval(resultTabs);
  await runScript(page, "DROP PROCEDURE IF EXISTS qa_sql_glued");
  assert.equal(error, null, `the script failed: ${error}`);
  assert.equal(status, "1 row");
  assert.equal(tabs.length, 3, JSON.stringify(tabs));
}
