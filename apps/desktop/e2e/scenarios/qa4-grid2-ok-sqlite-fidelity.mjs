// Regression guard (passes): SQLite values and names survive the data editor. Keys above 2^53 and
// the largest integer, REAL values with 17 digits, blobs, text with quotes, newlines and emoji, and
// identifiers with quotes, dots, brackets, emoji and keywords are written back exactly; a deleted row
// comes back from the undo script with the same storage class and value in every column (including
// an untyped column holding a REAL 5.0 and a text '007').
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { assert, button, connections, editBar, editorText, expand, focusCell, openData, previewSql, save, setCell, statusBar, visibleTab } from "../qa-grid.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ok.db"));
  db.exec(`CREATE TABLE "we""ird tab" ("order id" INTEGER PRIMARY KEY, "a""b" TEXT, "select" REAL, "x.y" BLOB, "[br]" TEXT, "💥" TEXT, any_col);
           INSERT INTO "we""ird tab" VALUES (9007199254740992, 'p', 0.1, x'00ff', 's', 't', 5.0),
             (9007199254740993, 'it''s' || char(10) || 'x "q" é😀', 0.1 + 0.2, x'', 's2', 't2', '007'),
             (9223372036854775807, 'p3', 1e300, NULL, '', NULL, x'0a');`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "ok.db") }]]);
}

export default async function (page, { dir }) {
  await expand(page, "QA LITE", null, 'we"ird tab');
  await openData(page, 'we"ird tab');
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  const db = new DatabaseSync(join(dir, "ok.db"));
  const dump = () => {
    const st = db.prepare(`SELECT "order id", typeof("a""b"), "a""b", typeof("select"), "select", typeof("x.y"), hex("x.y"), "[br]", "💥", typeof(any_col), any_col FROM "we""ird tab" ORDER BY 1`);
    st.setReadBigInts(true);
    return JSON.stringify(st.all(), (k, v) => (typeof v === "bigint" ? `${v}n` : v));
  };
  // Edits on the rows with keys above 2^53: the WHERE keeps every digit.
  await focusCell(page, 1, 1);
  await setCell(page, "edited 'q'");
  await focusCell(page, 1, 2);
  await setCell(page, "0.30000000000000004");
  await focusCell(page, 2, 3);
  await setCell(page, "\\x0aff");
  assert.match(await previewSql(page), /WHERE "order id" = 9007199254740993;\s*UPDATE .* WHERE "order id" = 9223372036854775807;$/s);
  assert.match(await save(page), /to edit a cell/);
  const rows = JSON.parse(dump());
  assert.equal(rows[0]["order id"], "9007199254740992n");
  assert.equal(rows[1]["a\"b"], "edited 'q'");
  assert.equal(rows[1].select, 0.30000000000000004);
  assert.equal(rows[2]["hex(\"x.y\")"], "0AFF");

  // Delete a row and run its undo script: the table is as it was.
  const before = dump();
  await page.sleep(500);
  await focusCell(page, 1, 1);
  await page.key("Delete", "Delete", 46);
  assert.match(await save(page), /to edit a cell/);
  await page.waitFor(`${statusBar}.includes('Undo script')`, 15_000);
  await page.eval(`${button(visibleTab, "Undo script")}.click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent.includes('INSERT')`, 15_000);
  const script = await page.eval(editorText);
  assert.equal(JSON.parse(dump()).length, 2);
  db.exec(script);
  assert.equal(dump(), before);
  db.close();
}
