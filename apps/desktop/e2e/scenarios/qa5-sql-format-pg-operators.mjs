// BUG QA5-SQL-01 (medium). The formatter (Ctrl+Shift+F) splits PostgreSQL operators it does not
// know into several, changing the statement: pgvector's `<->` and `<#>`, jsonb's `#-` and `@?`,
// and the pattern operators `~~*`, `!~~`, `!~~*`.
// Steps: PostgreSQL connection; type `select a <-> b, a <#> b, j #- '{x}', j @? '$.x', s !~~* 'q' from t`;
// Ctrl+Shift+F.
// Expected: every operator is kept as one token (`a <-> b`, `j #- '{x}'` ...), so the formatted
// statement still means the same.
// Actual: `a < -> b`, `a < #> b`, `j # -'{x}'`, `j @ ? '$.x'`, `s !~ ~* 'q'`: a syntax error or
// another operator (`# -'{x}'` is a prefix minus).
// Code: crates/platypus-sql/src/format.rs, OPERATORS (line ~196) lacks these multi-character
// operators, so `symbol()` falls back to one-character operators and `spaced()` puts spaces
// between them.
import { openEditor, visibleTab, editorText, needs, assert, connections, CTRL, SHIFT } from "../qa-grid.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  await openEditor(page, "QA PG", "pg");
  const sql = "select a <-> b, a <#> b, j #- '{x}', j @? '$.x', s !~~* 'q', s ~~* 'q', s !~~ 'q' from t";
  await page.eval(`(() => { const el = ${visibleTab}.querySelector('.cm-content'); const cv = el.cmView ?? el.cmTile; const v = cv.view ?? cv.root?.view ?? cv.rootView?.view; v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: ${JSON.stringify(sql)} }, selection: { anchor: 0 } }); v.focus(); })()`);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("F", "KeyF", 70, CTRL | SHIFT);
  await page.waitFor(`${editorText}.includes('\\n')`, 10_000);
  const after = await page.eval(editorText);
  for (const op of ["<->", "<#>", "#-", "@?", "!~~*", "~~*", "!~~"]) {
    assert.ok(after.includes(` ${op} `), `operator ${op} was split: ${JSON.stringify(after)}`);
  }
}
