// Regression guard (passes): the MongoDB editor. insertMany with extended types, find with
// sort/skip/limit, projection and regex, a script of statements without semicolons over
// several lines, a `;` inside a string, comments, distinct, and clear syntax errors.
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, resultError, resultTabs, runScript, runStatement, saveConnections, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("mongo");
export const setup = ({ dir }) => saveConnections(dir, ["mongo"]);

export default async function (page) {
  await openEditor(page, DB.mongo.name);
  await runStatement(page, "db.qa_sql_ok_m.drop()");
  assert.equal(
    await runStatement(
      page,
      `db.qa_sql_ok_m.insertMany([{ _id: 1, n: "a", v: NumberLong("9007199254740993"), d: ISODate("2024-01-01T00:00:00Z"), dec: NumberDecimal("1.10") }, { _id: 2, n: "b;c" }])`,
    ),
    "2 rows affected",
  );
  await runStatement(page, "db.qa_sql_ok_m.find({}).sort({ _id: -1 }).skip(1).limit(1)");
  assert.equal(await gridTsv(page), "_id\tn\tv\td\tdec\n1\ta\t9007199254740993\t2024-01-01 00:00:00+00:00\t1.10");
  await runStatement(page, "db.qa_sql_ok_m.find({ n: /^B/i }, { n: 1, _id: 0 })");
  assert.equal(await gridTsv(page), "n\nb;c");
  await runScript(page, "// a comment; with a semicolon\ndb.qa_sql_ok_m.find({\n  n: \"a\"\n})\ndb.qa_sql_ok_m.countDocuments({}) // trailing");
  assert.equal((await page.eval(resultTabs)).length, 2);
  assert.equal(await gridTsv(page), "count\n2");
  await runStatement(page, `db.qa_sql_ok_m.distinct("n")`);
  assert.equal(await gridTsv(page), "n\na\nb;c");
  assert.equal(await runStatement(page, "db.qa_sql_ok_m.fnd({})"), "Error");
  assert.match(await page.eval(resultError), /unknown collection method `fnd\(\)`/);
  await runStatement(page, "db.qa_sql_ok_m.drop()");
}
