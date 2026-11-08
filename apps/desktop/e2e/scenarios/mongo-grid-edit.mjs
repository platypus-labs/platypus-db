// Editing the documents of a MongoDB collection in the grid, on the standalone server of the
// `mongo` profile: a cell of each editable type is saved with the right BSON type (checked
// with `$type` on the server), a NULL cell writes `null`, a new document gets a generated
// `ObjectId` or the `_id` typed, a deleted one comes back whole from the undo script, the
// preview shows the exact mongosh, and a typed `find` (its database named or the tab's after
// `use`, a collection that needs `getCollection`, a projection) edits as well. Refused before
// anything runs: a change of `_id`, a column of mixed types, a field with a dot, values that do
// not fit the type. A view, an aggregation, a `findOne` and a projection without `_id` stay
// read-only. A document deleted meanwhile gives a clear error, and on a standalone server the
// error says how many commands were saved.
// Skipped when the standalone MongoDB is not running.
import assert from "node:assert/strict";
import { editorText, status, treeRow } from "../qa-sql.mjs";
import {
  closePreview,
  connection,
  count,
  deleteRow,
  discard,
  editBar,
  editCell,
  editable,
  expand,
  insertRow,
  isEditable,
  layout,
  mongosh,
  needs,
  openData,
  openFresh,
  openEditor,
  opensEditor,
  preview,
  previewOrError,
  runStatement,
  runUndo,
  save,
  saveConnections,
  saveFails,
  setNull,
  treeMenu,
} from "../mongo-grid-helpers.mjs";

const NAME = "E2E Mongo grid";
const C = "e2e_ge";

/** What an edit writes for whole number `n` in field `field`: the number in the numeric type the
 *  document stores, `fallback` (the column's type) when it is none of them. */
const whole = (field, n, fallback = String(n)) => {
  const is = (type) => `{ case: { $eq: [{ $type: "$${field}" }, "${type}"] }`;
  return `{ $switch: { branches: [${is("int")}, then: ${n} }, ${is("long")}, then: NumberLong(${n}) }, ` +
    `${is("double")}, then: ${n}.0 }, ${is("decimal")}, then: NumberDecimal("${n}") }], default: ${fallback} } }`;
};

/** The documents of the collection, as the scenario starts and as it puts them back. */
const SEED = `db.${C}.insertMany([
       { _id: 1, name: "Alpha", qty: NumberInt(1), big: NumberLong(10), price: 1.5, ok: false,
         born: ISODate("2024-01-02T03:04:05.678Z"), tags: ["a"], meta: { x: 1 }, dec: NumberDecimal("1.10") },
       { _id: 2, name: "Beta", qty: NumberInt(2), big: NumberLong(20), price: 2.5, ok: true,
         born: ISODate("2024-02-03T04:05:06.789Z"), tags: ["b"], meta: { x: 2 }, dec: NumberDecimal("2.20") },
       { _id: 3, name: "Gamma", qty: NumberInt(3), big: NumberLong(30), price: 3.5, ok: false,
         born: ISODate("2024-03-04T05:06:07.890Z"), tags: ["c"], meta: { x: 3 }, dec: NumberDecimal("3.30") },
     ]);`;

export const skip = needs("standalone");

export function setup({ dir }) {
  saveConnections(dir, connection(1, NAME, "standalone"));
  mongosh(
    "standalone",
    `db.${C}.drop(); db.${C}_view.drop(); db.${C}_mixed.drop(); db.${C}_dotted.drop(); db.getCollection("${C} space").drop();
     db.getSiblingDB("e2e_other").dropDatabase();
     ${SEED}
     db.createView("${C}_view", "${C}", []);
     db.${C}_mixed.insertMany([{ _id: 1, v: 1 }, { _id: 2, v: "two" }]);
     db.${C}_dotted.insertOne({ _id: 1, "a.b": 1, c: 2 });
     db.getCollection("${C} space").insertOne({ _id: 1, v: 1 });
     db.getSiblingDB("e2e_other").things.insertMany([{ _id: 1, v: "x" }, { _id: 2, v: "z" }]);`,
  );
}

/** Whether the collection holds exactly one document that matches `filter`. */
const has = (filter, collection = C) => count("standalone", collection, filter) === 1;

export default async function (page) {
  await expand(page, NAME, null, C);

  // The types of the columns, as the grid shows them.
  await openFresh(page, C, "3 rows");
  let g = await layout(page);
  assert.deepEqual(g.header, ["_id", "name", "qty", "big", "price", "ok", "born", "tags", "meta", "dec"]);

  // One cell: the preview is the mongosh that runs, and the undo script puts the value back.
  await editCell(page, g.row(1), g.col("name"), "Alpha 2");
  assert.match(await page.eval(editBar), /1 row changed/);
  assert.equal(await preview(page), `db.${C}.updateOne({ _id: 1 }, { $set: { name: "Alpha 2" } })`);
  await save(page, "3 rows");
  assert.ok(has(`{ _id: 1, name: "Alpha 2" }`));
  assert.equal(await runUndo(page), `db.${C}.updateOne({ _id: 1 }, { $set: { name: "Alpha" } })`);
  assert.ok(has(`{ _id: 1, name: "Alpha" }`));

  // A cell of every type, in one document: each is written with its column's BSON type.
  await openFresh(page, C, "3 rows");
  g = await layout(page);
  const row = g.row(2);
  await editCell(page, row, g.col("qty"), "7");
  await editCell(page, row, g.col("big"), "9007199254740993");
  await editCell(page, row, g.col("price"), "2");
  await editCell(page, row, g.col("ok"), "false");
  await editCell(page, row, g.col("born"), "2025-03-04 05:06:07.089+00:00");
  await editCell(page, row, g.col("tags"), '["x", 2]');
  await editCell(page, row, g.col("meta"), '{"y":{"z":1.5}}');
  await editCell(page, row, g.col("dec"), "2.50");
  const sql = await preview(page);
  // A changed number makes the update a pipeline: numbers keep the stored type, the rest are literals.
  assert.match(sql, new RegExp(`^db\\.${C}\\.updateOne\\(\\{ _id: 2 \\}, \\[\\{ \\$set: \\{ `));
  for (const piece of [
    `qty: ${whole("qty", 7)}`,
    'default: NumberLong("9007199254740993")',
    `price: ${whole("price", 2, "2.0")}`,
    "ok: { $literal: false }",
    'born: { $literal: ISODate("2025-03-04T05:06:07.089Z") }',
    'tags: { $literal: ["x", 2] }',
    "meta: { $literal: { y: { z: 1.5 } } }",
    'then: NumberDecimal("2.50") }], default: NumberDecimal("2.50")',
  ]) {
    assert.ok(sql.includes(piece), `${piece} in ${sql}`);
  }
  await save(page, "3 rows");
  assert.ok(
    has(`{ _id: 2, qty: { $type: "int" }, big: { $type: "long" }, price: { $type: "double" }, ok: { $type: "bool" },
          born: { $type: "date" }, tags: { $type: "array" }, meta: { $type: "object" }, dec: { $type: "decimal" } }`),
  );
  assert.ok(has(`{ _id: 2, qty: 7, big: NumberLong("9007199254740993"), price: 2, ok: false, born: ISODate("2025-03-04T05:06:07.089Z"), "tags.1": 2, "meta.y.z": 1.5, dec: NumberDecimal("2.50") }`));
  assert.ok(has(`{ _id: 1, name: "Alpha", qty: 1 }`), "another document changed");
  // The undo script writes the old values back, with their types.
  await runUndo(page);
  assert.ok(
    has(`{ _id: 2, qty: { $eq: 2, $type: "int" }, big: NumberLong(20), price: 2.5, ok: true, born: ISODate("2024-02-03T04:05:06.789Z"),
          tags: ["b"], meta: { x: 2 }, dec: NumberDecimal("2.20") }`),
  );

  // NULL writes `null` (not a missing field), numbers and text alike.
  await openFresh(page, C, "3 rows");
  g = await layout(page);
  await setNull(page, g.row(3), g.col("name"));
  await setNull(page, g.row(3), g.col("qty"));
  assert.match(await page.eval(editBar), /1 row changed/);
  const nulls = await preview(page);
  assert.match(nulls, new RegExp(`^db\\.${C}\\.updateOne\\(\\{ _id: 3 \\}, \\{ \\$set: \\{ (name: null, qty: null|qty: null, name: null) \\} \\}\\)$`));
  await save(page, "3 rows");
  assert.ok(has(`{ _id: 3, name: { $type: "null" }, qty: { $type: "null" } }`));
  await runUndo(page);
  assert.ok(has(`{ _id: 3, name: "Gamma", qty: { $type: "int" } }`));

  // A new document without an `_id`: the app makes an ObjectId, cells left NULL are not sent,
  // and the undo script deletes the document by that id.
  await openFresh(page, C, "3 rows");
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("name"), "New");
  await editCell(page, 0, g.col("qty"), "5");
  await editCell(page, 0, g.col("price"), "3");
  assert.match(
    await preview(page),
    new RegExp(`^db\\.${C}\\.insertOne\\(\\{ _id: ObjectId\\("[0-9a-f]{24}"\\), name: "New", qty: 5, price: 3\\.0 \\}\\)$`),
  );
  await save(page, "4 rows");
  assert.ok(has(`{ name: "New", _id: { $type: "objectId" }, qty: { $type: "int" }, price: { $type: "double" }, ok: { $exists: false } }`));
  assert.match(await runUndo(page), new RegExp(`^db\\.${C}\\.deleteOne\\(\\{ _id: ObjectId\\("[0-9a-f]{24}"\\) \\}\\)$`));
  assert.equal(count("standalone", C, `{ name: "New" }`), 0);

  // A new document with its `_id` typed, an existing `_id` refused by the server.
  await openFresh(page, C, "3 rows");
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("_id"), "50");
  await editCell(page, 0, g.col("name"), "Fifty");
  assert.equal(await preview(page), `db.${C}.insertOne({ _id: 50, name: "Fifty" })`);
  await save(page, "4 rows");
  assert.ok(has(`{ _id: 50, name: "Fifty" }`));
  assert.equal(await runUndo(page), `db.${C}.deleteOne({ _id: 50 })`);
  assert.equal(count("standalone", C), 3);
  await openFresh(page, C, "3 rows");
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("_id"), "1");
  await editCell(page, 0, g.col("name"), "Duplicate");
  assert.match(await preview(page), /^db\.e2e_ge\.insertOne\(\{ _id: 1, name: "Duplicate" \}\)$/);
  await saveFails(page, /duplicate key/i);
  assert.ok(has(`{ _id: 1, name: "Alpha" }`), "an existing document was replaced");
  await discard(page);

  // A deleted document comes back whole from the undo script (a note first), types included.
  await openFresh(page, C, "3 rows");
  g = await layout(page);
  await deleteRow(page, g.row(3));
  assert.equal(await preview(page), `db.${C}.deleteOne({ _id: 3 })`);
  await save(page, "2 rows");
  assert.equal(count("standalone", C, "{ _id: 3 }"), 0);
  const restore = await runUndo(page);
  assert.match(restore, /^\/\/ .*[\s\S]*\ndb\.e2e_ge\.insertOne\(\{ _id: 3, name: "Gamma", /);
  assert.ok(
    has(`{ _id: 3, name: "Gamma", qty: { $type: "int" }, big: NumberLong(30), price: 3.5, ok: false,
          born: ISODate("2024-03-04T05:06:07.890Z"), tags: ["c"], meta: { x: 3 }, dec: NumberDecimal("3.30") }`),
  );

  // Delete, update and insert in one save run in that order.
  await openFresh(page, C, "3 rows");
  g = await layout(page);
  await deleteRow(page, g.row(3));
  await editCell(page, g.row(1), g.col("qty"), "9");
  await insertRow(page);
  g = await layout(page);
  await editCell(page, 0, g.col("_id"), "60");
  await editCell(page, 0, g.col("name"), "Zed");
  assert.match(await page.eval(editBar), /3 rows changed/);
  assert.deepEqual((await preview(page)).split("\n"), [
    `db.${C}.deleteOne({ _id: 3 })`,
    `db.${C}.updateOne({ _id: 1 }, [{ $set: { qty: ${whole("qty", 9)} } }])`,
    `db.${C}.insertOne({ _id: 60, name: "Zed" })`,
  ]);
  await save(page, "3 rows");
  assert.equal(count("standalone", C, "{ _id: 3 }"), 0);
  assert.ok(has(`{ _id: 1, qty: 9 }`) && has(`{ _id: 60, name: "Zed" }`));
  await runUndo(page);
  assert.equal(count("standalone", C), 3);
  assert.ok(has(`{ _id: 3, name: "Gamma" }`) && has(`{ _id: 1, qty: 1 }`));

  await refused(page);
  await meanwhile(page);
  await typedFinds(page);
  await readOnlyResults(page);
}

/** Changes the app refuses before anything runs, each with its reason. */
async function refused(page) {
  const expectRefusal = async (change, re) => {
    const { sql, error } = await previewOrError(page);
    assert.equal(sql, null, `${change}: ${sql}`);
    assert.match(error, re, change);
    await discard(page);
  };

  await openFresh(page, C, "3 rows");
  let g = await layout(page);
  await editCell(page, g.row(1), g.col("_id"), "9");
  await expectRefusal("a new _id", /_id of a document cannot be changed/);

  g = await layout(page);
  await editCell(page, g.row(1), g.col("qty"), "many");
  await expectRefusal("text in an int", /column qty: invalid number/);
  await editCell(page, g.row(1), g.col("price"), "abc");
  await expectRefusal("text in a double", /column price: invalid number/);
  await editCell(page, g.row(1), g.col("born"), "yesterday");
  await expectRefusal("a date that is none", /column born: invalid date/);
  await editCell(page, g.row(1), g.col("dec"), "1,5");
  await expectRefusal("a decimal that is none", /column dec: invalid number/);
  await editCell(page, g.row(1), g.col("meta"), "[1]");
  await expectRefusal("an array in a document column", /not a document/);
  await editCell(page, g.row(1), g.col("tags"), '{"a":1}');
  await expectRefusal("a document in an array column", /not an array/);
  assert.ok(has(`{ _id: 1, name: "Alpha", qty: 1 }`), "a refused change was written");

  // A column whose values have different types: their text cannot tell a string from a number.
  await openFresh(page, `${C}_mixed`, "2 rows");
  g = await layout(page);
  await editCell(page, g.row(1), g.col("v"), "5");
  await expectRefusal("a mixed column", /several types/);

  // A field with a dot in its name would be read as a path by `$set`.
  await openFresh(page, `${C}_dotted`, "1 row");
  g = await layout(page);
  await editCell(page, g.row(1), g.col("a.b"), "5");
  await expectRefusal("a dotted field", /path or an operator/);
  await editCell(page, g.row(1), g.col("c"), "7");
  assert.equal(await preview(page), `db.${C}_dotted.updateOne({ _id: 1 }, [{ $set: { c: ${whole("c", 7)} } }])`);
  await closePreview(page);
  await discard(page);
  assert.equal(count("standalone", `${C}_dotted`, "{ c: 2 }"), 1);
}

/** A document deleted since the grid read it: a clear error, and what ran before stays. */
async function meanwhile(page) {
  await openFresh(page, C, "3 rows");
  let g = await layout(page);
  await editCell(page, g.row(1), g.col("name"), "Changed");
  mongosh("standalone", `db.${C}.deleteOne({ _id: 1 })`);
  assert.equal(await preview(page), `db.${C}.updateOne({ _id: 1 }, { $set: { name: "Changed" } })`);
  const shown = await saveFails(page, /Command 1/);
  assert.match(shown, /the document was not found/);
  assert.match(shown, /deleted, or its _id changed/);
  assert.match(shown, /Nothing was saved/);
  await discard(page);

  // The second command is the one that fails: on a standalone server the first stays, and the
  // error says so.
  await openFresh(page, C, "2 rows");
  g = await layout(page);
  await deleteRow(page, g.row(3));
  await editCell(page, g.row(2), g.col("name"), "Gone");
  mongosh("standalone", `db.${C}.deleteOne({ _id: 2 })`);
  assert.deepEqual((await preview(page)).split("\n"), [`db.${C}.deleteOne({ _id: 3 })`, `db.${C}.updateOne({ _id: 2 }, { $set: { name: "Gone" } })`]);
  const partial = await saveFails(page, /Command 2/);
  assert.match(partial, /The command before it was saved/);
  assert.match(partial, /no transactions/);
  assert.equal(count("standalone", C, "{ _id: 3 }"), 0, "the first command was saved");
  await discard(page);
  mongosh("standalone", `db.${C}.deleteMany({}); ${SEED}`);
}

/** A typed `find`: its database, a collection that needs `getCollection`, a projection. */
async function typedFinds(page) {
  await openEditor(page, NAME);

  // A filter, a sort and a limit: the documents of the collection, still edited by `_id`.
  assert.equal(await runStatement(page, `db.${C}.find({ qty: { $gt: 0 } }).sort({ _id: 1 }).limit(2)`), "2 rows");
  await editable(page, "2 rows");
  let g = await layout(page);
  await editCell(page, g.row(2), g.col("name"), "Typed");
  assert.equal(await preview(page), `db.${C}.updateOne({ _id: 2 }, { $set: { name: "Typed" } })`);
  await save(page, "2 rows");
  assert.ok(has(`{ _id: 2, name: "Typed" }`));
  mongosh("standalone", `db.${C}.updateOne({ _id: 2 }, { $set: { name: "Beta" } })`);

  // An inclusion projection that keeps `_id`: the columns it lists.
  assert.equal(await runStatement(page, `db.${C}.find({}, { name: 1, qty: 1 })`), "3 rows");
  await editable(page, "3 rows");
  g = await layout(page);
  assert.deepEqual(g.header, ["_id", "name", "qty"]);
  await editCell(page, g.row(3), g.col("qty"), "33");
  assert.equal(await preview(page), `db.${C}.updateOne({ _id: 3 }, [{ $set: { qty: ${whole("qty", 33)} } }])`);
  await save(page, "3 rows");
  assert.ok(has(`{ _id: 3, qty: 33, name: "Gamma", big: NumberLong(30) }`), "the fields the projection left out changed");
  mongosh("standalone", `db.${C}.updateOne({ _id: 3 }, { $set: { qty: NumberInt(3) } })`);

  // The database named in the statement: the preview names it too.
  assert.equal(await runStatement(page, `db.getSiblingDB("e2e_other").things.find({})`), "2 rows");
  await editable(page, "2 rows");
  g = await layout(page);
  await editCell(page, g.row(1), g.col("v"), "y");
  assert.equal(await preview(page), `db.getSiblingDB("e2e_other").things.updateOne({ _id: 1 }, { $set: { v: "y" } })`);
  await save(page, "2 rows");
  assert.equal(mongosh("standalone", `print(db.getSiblingDB("e2e_other").things.countDocuments({ _id: 1, v: "y" }))`), "1");

  // `use` changes the tab's database: the collection is that one's, not the navigator's.
  await runStatement(page, "use e2e_other");
  assert.equal(await runStatement(page, "db.things.find({})"), "2 rows");
  await editable(page, "2 rows");
  g = await layout(page);
  await editCell(page, g.row(2), g.col("v"), "w");
  assert.equal(await preview(page), `db.things.updateOne({ _id: 2 }, { $set: { v: "w" } })`);
  await save(page, "2 rows");
  assert.equal(mongosh("standalone", `print(db.getSiblingDB("e2e_other").things.countDocuments({ _id: 2, v: "w" }))`), "1");
  await runStatement(page, "use platypus");

  // A name that is not a plain identifier is written with `getCollection`.
  assert.equal(await runStatement(page, `db.getCollection("${C} space").find()`), "1 row");
  await editable(page, "1 row");
  g = await layout(page);
  await editCell(page, g.row(1), g.col("v"), "2");
  assert.equal(await preview(page), `db.getCollection("${C} space").updateOne({ _id: 1 }, [{ $set: { v: ${whole("v", 2)} } }])`);
  await save(page, "1 row");
  assert.ok(has(`{ _id: 1, v: 2 }`, `${C} space`));
}

/** Results that are not the plain documents of a collection stay read-only. */
async function readOnlyResults(page) {
  await openEditor(page, NAME);
  for (const statement of [
    // Not a `find`.
    [`db.${C}.aggregate([{ $match: {} }])`, "3 rows"],
    [`db.${C}.findOne({ _id: 1 })`, "1 row"],
    [`db.${C}.distinct("name")`, "3 rows"],
    // A projection that leaves `_id` out, or that is not a list of fields.
    [`db.${C}.find({}, { _id: 0, name: 1 })`, "3 rows"],
    [`db.${C}.find({}, { name: 0 })`, "3 rows"],
    [`db.${C}.find({}, { "meta.x": 1 })`, "3 rows"],
    // A view is read through its collection, not written.
    [`db.${C}_view.find({})`, "3 rows"],
  ]) {
    const [text, rows] = statement;
    assert.equal(await runStatement(page, text), rows, text);
    assert.equal(await isEditable(page, 2500), false, `${text} must not be editable`);
    assert.equal(await opensEditor(page, 0, 0), false, `${text}: a cell opened its editor`);
  }

  // The data viewer of a view says why.
  await page.eval(`${treeRow("Views")}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow(`${C}_view`)} !== undefined`, 15_000);
  await treeMenu(page, `${C}_view`, "Open data");
  await page.waitFor(`${status} === "3 rows" && /only tables can be edited/.test(${editBar})`, 20_000);
  assert.equal(await page.eval(`document.querySelector('.edit-bar button[aria-label="Preview SQL"]') === null`), true);
  assert.equal(count("standalone", C), 3);
  // Nothing was left open.
  assert.equal(await page.eval(`document.querySelector('.sql-preview') === null`), true);
  assert.ok((await page.eval(editorText)).startsWith("db."));
}
