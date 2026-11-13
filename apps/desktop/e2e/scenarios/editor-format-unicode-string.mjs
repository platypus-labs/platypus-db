// qa4-sql-1 (low). The formatter (Ctrl+Shift+F) splits PostgreSQL's Unicode escape string
// `U&'d\0061t'` into `U & 'd\0061t'`: the string becomes a bitwise AND of a column `U` and a
// string, so the formatted statement no longer runs. (Same family: SQL Server's `b !< 2`
// comes out as `b ! < 2`.)
// Steps: PostgreSQL editor, type `select U&'d\0061t' as u, 2 as n`, Ctrl+Shift+F, Ctrl+Enter.
// Expected: the literal is kept as `U&'d\0061t'` and the query returns "dat".
// Actual: the text has `U & 'd\0061t'` and the run fails ("column u does not exist").
// Code: crates/platypus-sql/src/format.rs tokenizer: `U&'` (and `E'`, `N'`, `B'`, `X'`) prefixes
//   are handled but not the `&` of a Unicode escape string.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { DB, editorText, openEditor, resultError, runStatement, saveConnections, setEditor, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page) {
  await openEditor(page, DB.pg.name);
  const lit = "U&'d" + String.fromCharCode(92) + "0061t'";
  await setEditor(page, `select ${lit} as u, 2 as n`);
  await page.key("F", "KeyF", 70, CTRL | 8);
  await page.waitFor(`${editorText}.includes('SELECT')`, 5000);
  const text = await page.eval(editorText);
  assert.ok(text.includes(lit), `the Unicode string is kept as typed, got:\n${text}`);
  await runStatement(page);
  assert.equal(await page.eval(resultError), null, "the formatted query still runs");
}
