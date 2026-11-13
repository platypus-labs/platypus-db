// The rest of mongosh in the editor: bulkWrite, findOneAnd…, index and statistics methods; an
// option the server must get (arrayFilters) changing what it should; an unknown option and
// JavaScript refused by name; `updateOne` refusing `multi`, which used to change every match.
// Then the Explain tab: a plan of a read as a tree, and of a delete that stays unapplied.
import assert from "node:assert/strict";
import { CTRL, SHIFT } from "../cdp.mjs";
import { explainTab, operations } from "../plans.mjs";
import { DB, clearStatus, gridTsv, openEditor, resultError, runStatement, saveConnections, setEditor, skipUnless } from "../editor-helpers.mjs";

const C = "qa_mongo_surface";

export const skip = () => skipUnless("mongo");
export const setup = ({ dir }) => saveConnections(dir, ["mongo"]);

export default async function (page) {
  await openEditor(page, DB.mongo.name);
  await runStatement(page, `db.${C}.drop()`);

  // bulkWrite, with a count of what it changed.
  assert.equal(
    await runStatement(
      page,
      `db.${C}.bulkWrite([
        { insertOne: { document: { _id: 1, n: 1, x: [1, 2, 3] } } },
        { insertOne: { document: { _id: 2, n: 2 } } },
        { updateOne: { filter: { _id: 2 }, update: { $set: { n: 20 } } } },
        { deleteOne: { filter: { _id: 9 } } },
      ])`,
    ),
    "3 rows affected",
  );

  // An option that reaches the server changes only what it says.
  await runStatement(page, `db.${C}.updateOne({ _id: 1 }, { $set: { 'x.$[e]': 0 } }, { arrayFilters: [{ e: { $gte: 2 } }] })`);
  await runStatement(page, `db.${C}.find({ _id: 1, x: [1, 0, 0] }, { _id: 1 })`);
  assert.equal(await gridTsv(page), "_id\n1");

  // findOneAnd… answers with the document, in the grid.
  await runStatement(page, `db.${C}.findOneAndUpdate({ _id: 2 }, { $inc: { n: 1 } }, { returnDocument: 'after', projection: { n: 1, _id: 0 } })`);
  assert.equal(await gridTsv(page), "n\n21");
  await runStatement(page, `db.${C}.findOneAndDelete({ _id: 2 }, { projection: { n: 1, _id: 0 } })`);
  assert.equal(await gridTsv(page), "n\n21");

  // Index and statistics methods.
  await runStatement(page, `db.${C}.createIndexes([{ n: 1 }])`);
  await runStatement(page, `db.${C}.getName()`);
  assert.equal(await gridTsv(page), `name\n${C}`);
  await runStatement(page, `db.${C}.isCapped()`);
  assert.equal(await gridTsv(page), "isCapped\nfalse");

  // Refused by name, and nothing runs: an option it does not know, JavaScript, `multi` on updateOne.
  for (const [statement, says] of [
    [`db.${C}.updateOne({}, { $set: { n: 9 } }, { nope: 1 })`, /`nope` is not supported/],
    [`db.${C}.updateOne({}, { $set: { n: 9 } }, { multi: true })`, /`multi` is not supported/],
    [`db.${C}.find().forEach(1)`, /not JavaScript/],
    [`db.${C}.watch()`, /take over the connection/],
  ]) {
    assert.equal(await runStatement(page, statement), "Error", statement);
    assert.match(await page.eval(resultError), says);
  }
  await runStatement(page, `db.${C}.find({ n: 9 })`);
  assert.equal(await gridTsv(page).catch(() => ""), "");

  // The Explain tab: the plan of a read as a tree of the server's stages.
  await setEditor(page, `db.${C}.find({ n: 1 })`);
  await clearStatus(page);
  await page.key("E", "KeyE", 69, CTRL | SHIFT);
  await page.waitFor(`${explainTab}?.querySelector('.explain-node') != null`, 20_000);
  const read = await page.eval(operations);
  assert.ok(read.includes("IXSCAN"), `the index is used: ${read}`);
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`${explainTab} === undefined`);

  // The plan of a write leaves the documents alone.
  await runStatement(page, `db.${C}.explain().deleteMany({})`);
  await runStatement(page, `db.${C}.countDocuments({})`);
  assert.equal(await gridTsv(page), "count\n1");
  await runStatement(page, `db.${C}.drop()`);
}
