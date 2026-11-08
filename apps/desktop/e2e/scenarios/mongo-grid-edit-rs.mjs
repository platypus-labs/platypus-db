// Saving the grid's changes to MongoDB documents on the single-node replica set (`mongo-rs`
// of the `mongo` profile): all or nothing. A save whose second command fails (a document
// deleted meanwhile, a duplicate `_id`) leaves the first unsaved; a good save commits its
// commands together and leaves the tab in autocommit; with a transaction open in the tab, the
// save runs in it, waits for Commit, and a failing save aborts the whole transaction and says
// so.
// Skipped when the replica set is not running.
import assert from "node:assert/strict";
import { visibleTab } from "../qa-sql.mjs";
import {
  connection,
  count,
  deleteRow,
  discard,
  editBar,
  editCell,
  editable,
  expand,
  insertRow,
  layout,
  mongosh,
  needs,
  openFresh,
  preview,
  save,
  saveConnections,
  saveFails,
  toolbarButton,
} from "../mongo-grid-helpers.mjs";

const NAME = "E2E Mongo rs grid";
const C = "e2e_rs";

export const skip = needs("replica");

/** Three documents, as every part of the scenario starts. */
const seed = () =>
  mongosh(
    "replica",
    `db.${C}.drop(); db.${C}.insertMany([{ _id: 1, name: "Alpha", qty: 1 }, { _id: 2, name: "Beta", qty: 2 }, { _id: 3, name: "Gamma", qty: 3 }]);`,
  );

/** Saves from the preview of the pending changes. */
async function saveChanges(page, rows) {
  await preview(page);
  await save(page, rows);
}

export function setup({ dir }) {
  saveConnections(dir, connection(2, NAME, "replica"));
  seed();
}

const names = () => mongosh("replica", `print(db.${C}.find().sort({ _id: 1 }).toArray().map(d => d._id + ":" + d.name).join(","))`);
const transactionOpen = `${visibleTab}.querySelector('.tx-open') !== null`;

export default async function (page) {
  await expand(page, NAME, null, C);

  // The second command is the one that fails (the document was deleted meanwhile): the first,
  // already run, is rolled back with it. The deletes run before the updates.
  await openFresh(page, C, "3 rows");
  let g = await layout(page);
  await deleteRow(page, g.row(3));
  await editCell(page, g.row(1), g.col("name"), "Changed");
  mongosh("replica", `db.${C}.deleteOne({ _id: 1 })`);
  assert.deepEqual((await preview(page)).split("\n"), [`db.${C}.deleteOne({ _id: 3 })`, `db.${C}.updateOne({ _id: 1 }, { $set: { name: "Changed" } })`]);
  const shown = await saveFails(page, /Command 2/);
  assert.match(shown, /the document was not found/);
  assert.match(shown, /Nothing was saved: MongoDB rolled the transaction back/);
  assert.equal(count("replica", C, "{ _id: 3 }"), 1, "the first command stayed saved");
  assert.equal(names(), "2:Beta,3:Gamma");
  await discard(page);
  // The tab went back to autocommit: it holds no transaction.
  assert.equal(await page.eval(transactionOpen), false);
  assert.ok(await page.eval(`${toolbarButton("Auto-commit")} !== null`));

  // A duplicate `_id` fails on the server, after a delete that had worked.
  seed();
  await openFresh(page, C, "3 rows");
  g = await layout(page);
  await deleteRow(page, g.row(3));
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("_id"), "2");
  await editCell(page, 0, g.col("name"), "Duplicate");
  assert.deepEqual((await preview(page)).split("\n"), [`db.${C}.deleteOne({ _id: 3 })`, `db.${C}.insertOne({ _id: 2, name: "Duplicate" })`]);
  assert.match(await saveFails(page, /duplicate key/i), /Command 2/);
  assert.equal(names(), "1:Alpha,2:Beta,3:Gamma", "a failed save changed documents");
  await discard(page);

  // Every command works: all are saved together, and the tab stays in autocommit.
  await openFresh(page, C, "3 rows");
  g = await layout(page);
  await deleteRow(page, g.row(3));
  await editCell(page, g.row(1), g.col("name"), "Alpha 2");
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("_id"), "4");
  await editCell(page, 0, g.col("name"), "Delta");
  await saveChanges(page, "3 rows");
  assert.equal(names(), "1:Alpha 2,2:Beta,4:Delta");
  assert.equal(await page.eval(transactionOpen), false);

  await inOpenTransaction(page);
}

/** With manual commit on, a save runs in the tab's transaction and waits for Commit. */
async function inOpenTransaction(page) {
  seed();
  await openFresh(page, C, "3 rows");
  await page.eval(`${toolbarButton("Auto-commit")}.click()`);
  await page.waitFor(`${toolbarButton("Manual commit")} !== null`, 10_000);
  let g = await layout(page);
  await editCell(page, g.row(2), g.col("name"), "Beta 2");
  await saveChanges(page, "3 rows");
  // Not committed: the tab shows a transaction open, and another session still sees the old name.
  await page.waitFor(transactionOpen, 15_000);
  assert.equal(names(), "1:Alpha,2:Beta,3:Gamma");
  await page.eval(`${toolbarButton("Commit")}.click()`);
  await page.waitFor(`!(${transactionOpen})`, 15_000);
  assert.equal(names(), "1:Alpha,2:Beta 2,3:Gamma");

  // The grid's read opens no transaction: the save does. A document deleted by another session
  // meanwhile is not found by it, and the transaction the save opened is rolled back.
  await openFresh(page, C, "3 rows");
  assert.equal(await page.eval(transactionOpen), false, "a read opened a transaction");
  g = await layout(page);
  await editCell(page, g.row(1), g.col("name"), "Alpha 2");
  mongosh("replica", `db.${C}.deleteOne({ _id: 1 })`);
  await preview(page);
  const missing = await saveFails(page, /Command 1/);
  assert.match(missing, /the document was not found/);
  assert.match(missing, /Nothing was saved: MongoDB rolled the transaction back/);
  await page.waitFor(`!(${transactionOpen})`, 15_000);
  assert.equal(names(), "2:Beta 2,3:Gamma");
  await discard(page);
  mongosh("replica", `db.${C}.insertOne({ _id: 1, name: "Alpha", qty: 1 })`);

  // Another save in a transaction, then one that fails (a duplicate `_id`): MongoDB aborts the
  // whole transaction, the first save with it, and the error says so.
  await openFresh(page, C, "3 rows");
  g = await layout(page);
  await editCell(page, g.row(3), g.col("name"), "Gamma 2");
  await saveChanges(page, "3 rows");
  await page.waitFor(transactionOpen, 15_000);
  assert.equal(names(), "1:Alpha,2:Beta 2,3:Gamma", "an uncommitted save is visible to others");
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("_id"), "2");
  await editCell(page, 0, g.col("name"), "Duplicate");
  await preview(page);
  const shown = await saveFails(page, /rolled back/);
  assert.match(shown, /Command 1: .*duplicate key/is);
  assert.match(shown, /the whole transaction was rolled back, with the changes you had not committed yet/);
  await page.waitFor(`!(${transactionOpen})`, 15_000);
  assert.equal(names(), "1:Alpha,2:Beta 2,3:Gamma", "the uncommitted save went with the transaction");
  await discard(page);
  assert.match(await page.eval(editBar), /to edit a cell/);

  // Back to autocommit, so that the next scenario starts clean.
  await page.eval(`${toolbarButton("Manual commit")}.click()`);
  await page.waitFor(`${toolbarButton("Auto-commit")} !== null`, 10_000);
}
