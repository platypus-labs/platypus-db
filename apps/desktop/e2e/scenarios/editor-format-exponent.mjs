// BUG QA5-SQL-02 (medium). The formatter (Ctrl+Shift+F) breaks a number written with a signed
// exponent: `1e-7` becomes `1e - 7` (a different expression, or a syntax error), `2E+3` becomes
// `2E + 3`. The unsigned `3e5` is kept. Same on PostgreSQL and SQLite.
// Steps: SQLite connection; type `select 1e-7 as a, 2E+3 as b, 3e5 as c`; Ctrl+Shift+F.
// Expected: the numbers are kept as written.
// Actual: `1e - 7`, `2E + 3`.
// Code: crates/platypus-sql/src/format.rs, symbol(): a number takes the following words and
// digits (`run_end`), but not the `+`/`-` after an `e`/`E`, so the sign becomes an operator.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { openEditor, visibleTab, editorText, assert, connections, CTRL, SHIFT } from "../grid-helpers.mjs";

export function setup({ dir }) {
  new DatabaseSync(join(dir, "p.db")).close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "p.db") }]]);
}

export default async function (page) {
  await openEditor(page, "QA LITE", null);
  const sql = "select 1e-7 as a, 2E+3 as b, 3e5 as c, 1.5e-3 as d";
  await page.eval(`(() => { const el = ${visibleTab}.querySelector('.cm-content'); const cv = el.cmView ?? el.cmTile; const v = cv.view ?? cv.root?.view ?? cv.rootView?.view; v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: ${JSON.stringify(sql)} }, selection: { anchor: 0 } }); v.focus(); })()`);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("F", "KeyF", 70, CTRL | SHIFT);
  await page.waitFor(`${editorText}.includes('\\n')`, 10_000);
  const after = await page.eval(editorText);
  for (const n of ["1e-7", "2E+3", "3e5", "1.5e-3"]) assert.ok(after.includes(n), `${n} was changed: ${JSON.stringify(after)}`);
}
