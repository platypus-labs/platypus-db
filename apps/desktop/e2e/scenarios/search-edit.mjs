// Editing the documents of an Elasticsearch index in the grid: the preview shows the requests
// (`_update` with a `doc`, a script for an object, `_create` for a document with an `_id`,
// `_doc` for one without, `DELETE`) in the order they run, saving runs them and reloads, the
// undo script puts documents back (the whole `_source` of a deleted one, the old values of an
// updated one, a note for a document whose `_id` the server made), an `_id` that exists is
// refused by the server with its reason, and values the mapping cannot take, or a change of
// an `_id`, are refused before anything runs.
// Skipped when Elasticsearch is not running.
import assert from "node:assert/strict";
import { gridTsv, runScript, editorText, status, visibleTab } from "../editor-helpers.mjs";
import {
  closePreview,
  connection,
  deleteRow,
  discard,
  drop,
  editBar,
  cellEditorKind,
  editCell,
  expandCluster,
  insertRow,
  openIndex,
  preview,
  save,
  saveConnections,
  seed,
  shownError,
  skip as skipSearch,
  sources,
  table,
} from "../search-helpers.mjs";

const NAME = "E2E ES edit";
const MAPPING = {
  properties: {
    name: { type: "keyword" },
    qty: { type: "long" },
    ok: { type: "boolean" },
    meta: { properties: { x: { type: "long" }, y: { type: "long" } } },
  },
};
const DOCS = () => [
  ["a", { name: "Alpha", qty: 1, ok: false, meta: { x: 1 } }],
  ["b", { name: "Beta", qty: 2, ok: true, meta: { x: 2, y: 20 } }],
  ["c", { name: "Gamma", qty: 3, ok: true }],
];

export const skip = skipSearch;
export const setup = async ({ dir }) => {
  saveConnections(dir, connection(40, NAME));
  for (const index of ["e2e_ed_a", "e2e_ed_b", "e2e_ed_c"]) await seed(index, MAPPING, DOCS());
};

/** The grid as `{ header, rows }`, and where a document and a column are. */
async function layout(page) {
  const [header, ...rows] = table(await gridTsv(page));
  return {
    header,
    rows,
    col: (name) => {
      assert.ok(header.includes(name), `${name} in ${header}`);
      return header.indexOf(name);
    },
    row: (id) => {
      const at = rows.findIndex((r) => r[0] === id);
      assert.ok(at >= 0, `${id} in ${rows.map((r) => r[0])}`);
      return at;
    },
  };
}

/** The undo script the app offers after a save, opened in its own tab. */
async function openUndo(page) {
  await page.waitFor(`[...${visibleTab}.querySelectorAll('button')].some(b => b.textContent.includes('Undo script'))`, 15_000);
  const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`[...${visibleTab}.querySelectorAll('button')].find(b => b.textContent.includes('Undo script')).click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${before}`, 10_000);
  await page.waitFor(`${editorText}.length > 0`, 10_000);
  return page.eval(editorText);
}

/** Waits for an error that matches `re` to show. */
const wait = (page, re) => page.waitFor(`${re}.test(${shownError})`, 10_000);

export default async function (page) {
  await expandCluster(page, NAME, "e2e_ed_a");

  // Several fields of one document: one `_update`, the keys in name order; the undo script
  // restores the old values (a request per document), and runs.
  const original = await sources("e2e_ed_a");
  await openIndex(page, "e2e_ed_a", "3 rows");
  let g = await layout(page);
  await editCell(page, g.row("a"), g.col("name"), "Alpha 2");
  await editCell(page, g.row("a"), g.col("qty"), "41");
  await editCell(page, g.row("a"), g.col("ok"), "true");
  assert.match(await page.eval(editBar), /1 row changed/);
  assert.equal(await preview(page), 'POST /e2e_ed_a/_update/a?refresh=wait_for\n{"doc":{"name":"Alpha 2","ok":true,"qty":41}}');
  await save(page, "3 rows");
  const changed = (await sources("e2e_ed_a")).a;
  assert.deepEqual([changed.name, changed.qty, changed.ok], ["Alpha 2", 41, true]);
  assert.deepEqual((await sources("e2e_ed_a")).b, original.b, "another document changed");
  const undo = await openUndo(page);
  assert.match(undo, /^POST \/e2e_ed_a\/_update\/a\?refresh=wait_for\n\{"doc":\{"name":"Alpha","ok":false,"qty":1\}\}/);
  await runScript(page);
  assert.deepEqual(await sources("e2e_ed_a"), original);

  // The same index: a deleted document comes back whole with the undo script.
  await openIndex(page, "e2e_ed_c", "3 rows");
  const before = await sources("e2e_ed_c");
  g = await layout(page);
  await deleteRow(page, g.row("b"));
  assert.equal(await preview(page), "DELETE /e2e_ed_c/_doc/b?refresh=wait_for");
  await save(page, "2 rows");
  assert.equal((await sources("e2e_ed_c")).b, undefined);
  const restore = await openUndo(page);
  assert.match(restore, /^PUT \/e2e_ed_c\/_create\/b\?refresh=wait_for\n\{"meta":\{"x":2,"y":20\},"name":"Beta","ok":true,"qty":2\}/);
  await runScript(page);
  assert.deepEqual(await sources("e2e_ed_c"), before);

  // An object replaces the old one (it does not merge), through a script.
  await openIndex(page, "e2e_ed_b", "3 rows");
  g = await layout(page);
  await editCell(page, g.row("b"), g.col("meta"), '{"y":2}');
  const script = await preview(page);
  assert.match(script, /^POST \/e2e_ed_b\/_update\/b\?refresh=wait_for\n/);
  assert.match(script, /"script"/);
  assert.match(script, /"set":\{"meta":\{"y":2\}\}/);
  await save(page, "3 rows");
  assert.deepEqual((await sources("e2e_ed_b")).b.meta, { y: 2 });

  // A new document with an `_id` is created (never replaces); the new row goes on top.
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("_id"), "fixed");
  await editCell(page, 0, g.col("name"), "New");
  await editCell(page, 0, g.col("qty"), "7");
  assert.equal(await preview(page), 'PUT /e2e_ed_b/_create/fixed?refresh=wait_for\n{"name":"New","qty":7}');
  await save(page, "4 rows");
  assert.deepEqual((await sources("e2e_ed_b")).fixed, { name: "New", qty: 7 });

  // Without an `_id` the server makes one, and the undo script can only say so.
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("name"), "Generated");
  assert.equal(await preview(page), 'POST /e2e_ed_b/_doc?refresh=wait_for\n{"name":"Generated"}');
  await save(page, "5 rows");
  const all = await sources("e2e_ed_b");
  const made = Object.entries(all).filter(([, d]) => d.name === "Generated");
  assert.equal(made.length, 1);
  assert.ok(made[0][0].length > 5, `a generated id: ${made[0][0]}`);
  assert.match(await openUndo(page), /^# the new document of e2e_ed_b has a generated _id: delete it by hand/);

  // Back to the tab of the data: delete, update and insert in one save run in that order.
  await openIndex(page, "e2e_ed_b", "5 rows");
  g = await layout(page);
  await deleteRow(page, g.row("c"));
  await editCell(page, g.row("a"), g.col("qty"), "9");
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("_id"), "z");
  await editCell(page, 0, g.col("name"), "Zed");
  assert.match(await page.eval(editBar), /3 rows changed/);
  const order = (await preview(page)).split("\n\n");
  assert.deepEqual(order, [
    "DELETE /e2e_ed_b/_doc/c?refresh=wait_for",
    'POST /e2e_ed_b/_update/a?refresh=wait_for\n{"doc":{"qty":9}}',
    'PUT /e2e_ed_b/_create/z?refresh=wait_for\n{"name":"Zed"}',
  ]);
  await save(page, "5 rows");
  const now = await sources("e2e_ed_b");
  assert.equal(now.c, undefined);
  assert.equal(now.a.qty, 9);
  assert.equal(now.z.name, "Zed");

  // An `_id` that exists is refused by the server with its reason, and nothing is replaced.
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("_id"), "a");
  await editCell(page, 0, g.col("name"), "Duplicate");
  assert.match(await preview(page), /^PUT \/e2e_ed_b\/_create\/a\?/);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await wait(page, /version.conflict|already exists/i);
  assert.equal((await sources("e2e_ed_b")).a.name, "Alpha", "an existing document was replaced");
  await discard(page);

  // Values the mapping cannot take, and a change of `_id`, are refused before anything runs.
  g = await layout(page);
  await editCell(page, g.row("a"), g.col("qty"), "many");
  assert.equal(await preview(page), null);
  await wait(page, /invalid integer/i);
  await discard(page);
  g = await layout(page);
  // A boolean cannot take free text: its editor is a select of the boolean values and null only.
  const okEditor = await cellEditorKind(page, g.row("a"), g.col("ok"));
  assert.equal(okEditor.tag, "SELECT");
  assert.deepEqual(okEditor.options.filter((v) => !v.startsWith("\0")).sort(), ["false", "true"]);
  assert.equal(okEditor.options.filter((v) => v.startsWith("\0")).length, 1, "null is the one other option");
  await assert.rejects(editCell(page, g.row("a"), g.col("ok"), "maybe"), /not an option/);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
  g = await layout(page);
  await editCell(page, g.row("a"), g.col("_id"), "renamed");
  assert.equal(await preview(page), null);
  await wait(page, /_id of a document can not be changed/i);
  await discard(page);
  assert.equal((await sources("e2e_ed_b")).a.qty, 9);
  assert.equal(await page.eval(status), "5 rows");
  await closePreviewIfOpen(page);
  await drop("e2e_ed_a", "e2e_ed_b", "e2e_ed_c");
}

async function closePreviewIfOpen(page) {
  if (await page.eval(`document.querySelector('.sql-preview') !== null`)) await closePreview(page);
}
