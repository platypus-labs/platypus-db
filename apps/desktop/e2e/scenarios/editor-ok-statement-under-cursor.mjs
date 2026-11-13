// Regression guard (passes): Ctrl+Enter runs the statement the cursor is in (SQLite). With the cursor
// before, inside or right after a statement, in blank lines between statements, after comments,
// next to strings or quoted names holding `;`, `--` or `/* */`, doubled quotes, an emoji, CRLF line
// ends and empty statements (`;;`), the statement that runs is the expected one. `|` marks the cursor.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { assert, clearStatus, connections, idle, openEditor, rows, stubClipboard, visibleTab, CTRL } from "../grid-helpers.mjs";

const CASES = [
  ["select 1 as a; select 2 as a;|", "2"],
  ["select 1 as a;| select 2 as a;", "1"],
  ["select 1 as a; |select 2 as a;", "2"],
  ["select 1 as a;\n\n|\n\nselect 2 as a;", "1"],
  ["select 'a;b' as a; select 2;|", "2"],
  ["select 'a;|b' as a; select 2;", "a;b"],
  ["select 1 -- x;y\n, 2 as b|; select 3", "1\t2"],
  ["select /* ; */ 7 |as a; select 3", "7"],
  ["select 'it''s;' as a|; select 3", "it's;"],
  ["select \"a;b\" as c from (select 1 as \"a;b\")|; select 3", "1"],
  ["select 'é😀;x' as a|; select 3", "é😀;x"],
  ["select '😀😀😀😀' as a; select 'z' as b|;", "z"],
  ["select 1 as a;\r\nselect 2 as a|;\r\nselect 3", "2"],
  ["select 4 as a; ;|; select 5", "4"],
  ["|select 4 as a", "4"],
  ["select 4 as a|  ", "4"],
  ["select 1 as a\n-- c\n;|\nselect 2 as a", "1"],
  ["select 1 as a;\n-- comment only\n|\nselect 2 as a", "1"],
  ["select 6 as a; -- tail|", "6"],
  ["-- head\nselect 8 as a|", "8"],
];

export function setup({ dir }) {
  new DatabaseSync(join(dir, "p.db")).close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "p.db") }]]);
}

export default async function (page) {
  const out = [];
  await openEditor(page, "QA LITE", null);
  await stubClipboard(page);
  for (const [text, expected] of CASES) {
    const at = text.indexOf("|");
    const sql = text.replace("|", "");
    await page.eval(`(() => { const el = ${visibleTab}.querySelector('.cm-content'); const cv = el.cmView ?? el.cmTile; const v = cv.view ?? cv.root?.view ?? cv.rootView?.view; v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: ${JSON.stringify(sql)} }, selection: { anchor: ${at} } }); v.focus(); })()`);
    await page.sleep(150);
    await clearStatus(page);
    await page.key("Enter", "Enter", 13, CTRL);
    await idle(page);
    const got = (await rows(page))[0]?.join("\t");
    if (got !== expected) out.push(`${JSON.stringify(text)}: expected ${JSON.stringify(expected)} got ${JSON.stringify(got)}`);
  }
  assert.equal(out.join("\n"), "");
}
