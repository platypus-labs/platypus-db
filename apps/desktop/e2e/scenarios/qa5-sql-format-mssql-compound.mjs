// BUG QA5-SQL-03 (medium). On SQL Server the formatter (Ctrl+Shift+F) splits the compound
// assignment operators `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=` and writes MERGE's
// `$action` as `$ action`: `SET @x += 2` becomes `SET @x + = 2`, a syntax error.
// Steps: SQL Server connection; type `declare @x int = 1; set @x += 2; set @x -= 1; set @x *= 3; select @x`;
// Ctrl+Shift+F.
// Expected: `SET @x += 2;` ... the operators are kept.
// Actual: `SET @x + = 2;`.
// Code: crates/platypus-sql/src/format.rs, OPERATORS (line ~196) has no compound-assignment
// operators (nor `$action`, handled by the "$" arm of symbol() only for `$1` and `${`).
import { openEditor, visibleTab, editorText, needs, assert, connections, CTRL, SHIFT } from "../qa-grid.mjs";

export const skip = needs("mssql");

export function setup({ dir }) {
  connections(dir, [["QA MSSQL", "mssql"]]);
}

export default async function (page) {
  await openEditor(page, "QA MSSQL", "mssql");
  const sql = "declare @x int = 1; set @x += 2; set @x -= 1; set @x *= 3; set @x /= 2; set @x %= 5; set @x |= 1; select @x";
  await page.eval(`(() => { const el = ${visibleTab}.querySelector('.cm-content'); const cv = el.cmView ?? el.cmTile; const v = cv.view ?? cv.root?.view ?? cv.rootView?.view; v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: ${JSON.stringify(sql)} }, selection: { anchor: 0 } }); v.focus(); })()`);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("F", "KeyF", 70, CTRL | SHIFT);
  await page.waitFor(`${editorText}.includes('\\n')`, 10_000);
  const after = await page.eval(editorText);
  for (const op of ["+=", "-=", "*=", "/=", "%=", "|="]) {
    assert.ok(after.includes(` ${op} `), `operator ${op} was split: ${JSON.stringify(after)}`);
  }
}
