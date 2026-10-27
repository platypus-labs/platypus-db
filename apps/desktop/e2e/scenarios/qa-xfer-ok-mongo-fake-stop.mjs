// Regression guard (passes): fake documents for a MongoDB collection with a unique index,
// stopped while running: the notice gives the number of documents written, which is what the
// collection holds (MongoDB keeps them: no transaction), and the unique field took distinct
// values, the numbers of a `long` field stayed NumberLong.
// Needs MongoDB from docker/compose.yml.
import assert from "node:assert/strict";
import { button, closeModal, db, dbQuiet, DBS, fill, generateDialog, menu, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

const COLLECTION = "qa_xfer_ok_mfake";

export const skip = () => skipUnless("mongo");

export function setup({ dir }) {
  saveConnections(dir, ["mongo"]);
  db(
    "mongo",
    `db.${COLLECTION}.drop(); db.${COLLECTION}.createIndex({ code: 1 }, { unique: true });
     db.${COLLECTION}.insertOne({ _id: 1, code: 'c1', n: NumberLong('5'), tags: ['a'], addr: { city: 'Roma' } }); 'ok'`,
  );
}

export default async function (page) {
  try {
    await showTable(page, DBS.mongo.name, "platypus", COLLECTION);
    await menu(page, COLLECTION, "Generate data", generateDialog, [DBS.mongo.name, "platypus"]);
    await page.waitFor(`${generateDialog}.querySelectorAll('select').length === 5`, 20_000);
    assert.deepEqual(await page.eval(`[...${generateDialog}.querySelectorAll('select')].map(s => s.value)`), ["next_key", "text", "integer", "word_list", "document"]);
    await page.eval(fill(`${generateDialog}.querySelector('input[type=number]')`, "100000"));
    await page.eval(`${button(generateDialog, "Generate")}.click()`);
    const stop = button(generateDialog, "Stop");
    await page.waitFor(`${stop} && !${stop}.disabled && /Adding… [1-9]/.test(${generateDialog}.textContent)`, 30_000);
    await page.eval(`${stop}.click()`);
    await page.waitFor(`/Stopped after/.test(${generateDialog}.textContent)`, 60_000);
    const written = Number((await page.eval(`${generateDialog}.textContent`)).match(/Stopped after ([\d.,]+) documents/)[1].replace(/[.,]/g, ""));
    await closeModal(page, generateDialog);
    assert.ok(written > 0 && written < 100000, `${written}`);
    assert.equal(db("mongo", `db.${COLLECTION}.countDocuments()`), String(written + 1));
    assert.equal(db("mongo", `db.${COLLECTION}.distinct('code').length`), String(written + 1));
    assert.equal(db("mongo", `db.${COLLECTION}.countDocuments({ n: { $type: 'long' } })`), String(written + 1));
  } finally {
    dbQuiet("mongo", `db.${COLLECTION}.drop()`);
  }
}
