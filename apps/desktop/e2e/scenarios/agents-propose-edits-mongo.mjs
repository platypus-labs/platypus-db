// Documents an agent proposes with propose_edits on a MongoDB collection (the single-node replica
// set of the `mongo` profile) open in the collection's data viewer of the open app as pending
// edits, found by `_id`: the viewer's statement is a `find` of the proposed `_id`s, an ObjectId
// sent in upper case finds its document, a number `_id` is read as the number the collection
// stores. Nothing changes until Save, which writes the ordinary mongosh of the grid; Discard
// leaves the collection as it was; a document that is gone is said so while the rest applies, and
// a save that fails on a document deleted meanwhile saves nothing (all or nothing). Refused by the
// MCP server: a view, a key other than `_id`, an `_id` that is not of the collection's type, a
// change of `_id`, an `_id` of mixed types, a value that does not fit, a field the collection does
// not have, the same document twice, a read-only connection. A tampered proposal in the profile's
// file with hostile text in an `_id`, a collection or a field name runs nothing.
// Skipped when the replica set or the MCP server is not there.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";
import { editorText, status, visibleTab } from "../editor-helpers.mjs";
import { connection, discard, editBar, mongosh, needs, preview, save, saveConnections, saveFails } from "../mongo-grid-helpers.mjs";

const NAME = "E2E Mongo propose";
const LOCKED = "E2E Mongo propose locked";
/** The id `connection(1, …)` gives the saved connection. */
const ID = "00000000-0000-4000-8000-00000000f001";
const C = "e2e_pe";
const A = "65a1b2c3d4e5f60718293a01";
const B = "65a1b2c3d4e5f60718293a02";
const G = "65a1b2c3d4e5f60718293a03";
/** An ObjectId no document has. */
const GHOST = "65a1b2c3d4e5f60718293aff";

const notice = `(${visibleTab}?.querySelector('.edit-notice')?.textContent ?? '')`;
const tabTitled = (title) =>
  `[...document.querySelectorAll('[role="tab"]')].some(t => t.textContent.includes(${JSON.stringify(title)}))`;
const toasts = `[...document.querySelectorAll('.toaster .toast')].map(t => t.textContent).join(' | ')`;

export const skip = async () => noMcp() ?? (await needs("replica")());

/** The documents every part of the scenario starts from. */
function seed() {
  mongosh(
    "replica",
    `db.${C}.drop(); db.${C}_n.drop(); db.${C}_view.drop(); db.${C}_mixed.drop();
     db.${C}.insertMany([
       { _id: ObjectId("${A}"), name: "Alpha", qty: 1 },
       { _id: ObjectId("${B}"), name: "Beta", qty: 2 },
       { _id: ObjectId("${G}"), name: "Gamma", qty: 3 },
     ]);
     db.${C}_n.insertMany([{ _id: 1, name: "One" }, { _id: 2, name: "Two" }, { _id: 3, name: "Three" }]);
     db.createView("${C}_view", "${C}", []);
     db.${C}_mixed.insertMany([{ _id: 1, v: 1 }, { _id: "two", v: 2 }]);`,
  );
}

export function setup({ dir }) {
  saveConnections(dir, connection(1, NAME, "replica"), connection(2, LOCKED, "replica", "read_only = true"));
  seed();
}

/** The documents of `collection` as `_id:name`, sorted by name. */
const names = (collection = C) =>
  mongosh("replica", `print(db.${collection}.find().sort({ name: 1 }).toArray().map(d => d._id + ":" + d.name).join(","))`);
const ORIGINAL = `${A}:Alpha,${B}:Beta,${G}:Gamma`;

/** Proposes through the MCP server and waits for the tab the app opens for it. */
async function propose(page, mcp, args, title) {
  const r = await mcp.call("propose_edits", { connection: NAME, schema: "platypus", table: C, title, ...args });
  assert.equal(r.error, false, r.text);
  assert.match(r.text, /Nothing changed/);
  await opened(page, title);
  return r.text;
}

/** Waits for the tab titled `title`. More than a few proposals in a minute are asked about
 *  first: the question is answered yes. */
async function opened(page, title) {
  const asked = `document.querySelector('#confirm-message') !== null`;
  await page.waitFor(`${tabTitled(title)} || ${asked}`, 30_000);
  if (await page.eval(asked)) {
    await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Open them').click()`);
  }
  await page.waitFor(tabTitled(title), 30_000);
}

/** Writes a proposal into the profile's file as a program other than the app would. */
function tamper(dir, edits, title) {
  const db = new DatabaseSync(join(dir, "agent-calls.db"));
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    db.prepare(
      "INSERT INTO proposals (agent, connection_id, title, sql, edits, created_at) VALUES (?, ?, ?, '', ?, ?)",
    ).run("intruder", ID, title, JSON.stringify(edits), Date.now());
  } finally {
    db.close();
  }
}

/** Waits until the app took every proposal waiting in the file. */
async function taken(page, dir) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    const db = new DatabaseSync(join(dir, "agent-calls.db"));
    db.exec("PRAGMA busy_timeout = 10000");
    const waiting = db.prepare("SELECT count(*) AS n FROM proposals").get().n;
    db.close();
    if (waiting === 0) return;
    await page.sleep(300);
  }
  assert.fail("the app did not take the proposal");
}

/** ObjectIds (one in upper case): the three kinds pending, the exact mongosh, saved. */
async function threeKinds(page, mcp, shots) {
  const text = await propose(
    page,
    mcp,
    {
      inserts: [{ name: "Delta", qty: 4 }],
      updates: [{ key: { _id: A.toUpperCase() }, set: { name: "Alpha 2" } }],
      deletes: [{ _id: G }],
    },
    "Tidy documents",
  );
  assert.match(text, /1 rows to insert, 1 to update, 1 to delete/);
  await page.waitFor(`${editBar}.includes('3 rows changed (1 modified, 1 new, 1 deleted)')`, 30_000);
  assert.equal(await page.eval(notice), "", "nothing was left out");
  // The viewer reads the two documents named, by their ObjectIds in lower case.
  assert.equal(
    await page.eval(editorText),
    `db.getSiblingDB("platypus").${C}.find({ _id: { $in: [ObjectId("${A}"), ObjectId("${G}")] } })`,
  );
  assert.equal(await page.eval(status), "2 rows");
  assert.equal(names(), ORIGINAL, "opening the proposal wrote nothing");
  if (shots) await page.screenshot(join(shots, "propose-edits-mongo-pending.png"));

  const lines = (await preview(page)).split("\n");
  assert.equal(lines.length, 3, lines.join("\n"));
  assert.equal(lines[0], `db.${C}.deleteOne({ _id: ObjectId("${G}") })`);
  assert.equal(lines[1], `db.${C}.updateOne({ _id: ObjectId("${A}") }, { $set: { name: "Alpha 2" } })`);
  assert.match(lines[2], new RegExp(`^db\\.${C}\\.insertOne\\(\\{ _id: ObjectId\\("[0-9a-f]{24}"\\), name: "Delta", qty: 4 \\}\\)$`));
  assert.equal(names(), ORIGINAL, "previewing writes nothing");
  // The viewer reads its documents again: the deleted one is gone.
  await save(page, "1 row");
  const saved = names().split(",");
  assert.deepEqual(saved.filter((d) => !d.endsWith(":Delta")), [`${A}:Alpha 2`, `${B}:Beta`]);
  assert.equal(saved.length, 3);
  // Not applied a second time once the rows are read again.
  await page.sleep(1500);
  assert.match(await page.eval(editBar), /to edit a cell/);
}

/** Number `_id`s are read as the numbers the collection stores; Discard writes nothing. */
async function numbersAndDiscard(page, mcp) {
  await propose(
    page,
    mcp,
    { table: `${C}_n`, updates: [{ key: { _id: 2 }, set: { name: "Deux" } }], deletes: [{ _id: "3" }] },
    "Numbered",
  );
  await page.waitFor(`${editBar}.includes('2 rows changed (1 modified, 1 deleted)')`, 30_000);
  assert.equal(await page.eval(editorText), `db.getSiblingDB("platypus").${C}_n.find({ _id: { $in: [2, 3] } })`);
  await discard(page);
  await page.sleep(500);
  assert.equal(names(`${C}_n`), "1:One,3:Three,2:Two");
}

/** A document that is gone is said so, the rest applies; a save that fails on a document
 *  deleted meanwhile saves nothing. */
async function partlyStale(page, mcp) {
  seed();
  await propose(
    page,
    mcp,
    {
      updates: [
        { key: { _id: B }, set: { name: "Beta 2" } },
        { key: { _id: GHOST }, set: { name: "Ghost" } },
      ],
      deletes: [{ _id: A }],
    },
    "Partly stale",
  );
  await page.waitFor(`${notice}.includes('1 proposed row could not be applied')`, 30_000);
  assert.match(await page.eval(notice), /no row of the table has that key/);
  assert.match(await page.eval(editBar), /2 rows changed \(1 modified, 1 deleted\)/);

  // Beta goes behind the app's back: the delete of Alpha, which ran first, is rolled back.
  mongosh("replica", `db.${C}.deleteOne({ _id: ObjectId("${B}") })`);
  await preview(page);
  const shown = await saveFails(page, /Command 2/);
  assert.match(shown, /the document was not found/);
  assert.match(shown, /Nothing was saved/);
  assert.equal(names(), `${A}:Alpha,${G}:Gamma`);
  await discard(page);
}

/** What the MCP server refuses, saying why; nothing is kept and nothing changes. */
async function refusals(mcp) {
  seed();
  const refused = async (args, what) => {
    const r = await mcp.call("propose_edits", { connection: NAME, schema: "platypus", table: C, ...args });
    assert.equal(r.error, true, r.text);
    assert.match(r.text, what);
  };
  await refused({ table: `${C}_view`, deletes: [{ _id: A }] }, /is a view/);
  await refused({ deletes: [{ name: "Beta" }] }, /not exactly the columns of a key/);
  await refused({ deletes: [{ _id: "7" }] }, /ObjectId/);
  await refused({ updates: [{ key: { _id: A }, set: { _id: B } }] }, /never changes/);
  await refused({ table: `${C}_mixed`, deletes: [{ _id: 1 }] }, /several types/);
  await refused({ updates: [{ key: { _id: B }, set: { qty: "many" } }] }, /invalid number/);
  await refused({ inserts: [{ nope: 1 }] }, /which the table does not have/);
  await refused({ updates: [{ key: { _id: A }, set: { name: "x" } }], deletes: [{ _id: A.toUpperCase() }] }, /named twice/);
  await refused({ updates: [{ key: { _id: { $oid: A } }, set: { name: "x" } }] }, /is an object/);
  const locked = await mcp.call("propose_edits", { connection: LOCKED, schema: "platypus", table: C, deletes: [{ _id: A }] });
  assert.equal(locked.error, true, locked.text);
  assert.match(locked.text, /read-only/);
  assert.equal(names(), ORIGINAL);
}

/** Tampered proposals in the profile's file: refused with a reason, no tab, nothing run. */
async function tampered(page, dir) {
  const base = { schema: "platypus", table: C, inserts: [], updates: [], deletes: [] };
  const attacks = [
    [{ ...base, deletes: [[["_id", `"); db.dropDatabase(); //`]]] }, "ObjectId (24 hexadecimal characters)"],
    [{ ...base, table: `${C}").drop(); db.x.find("`, deletes: [[["_id", A]]] }, "does not exist"],
    [{ ...base, table: `${C}_view`, deletes: [[["_id", A]]] }, "is a view"],
    [{ ...base, deletes: [[["name", "Beta"]]] }, "not exactly the columns of a key"],
    [{ ...base, updates: [{ key: [["_id", B]], set: [["$where", "1"]] }] }, "which the table does not have"],
  ];
  for (const [i, [edits, why]] of attacks.entries()) {
    tamper(dir, edits, `Intruder ${i}`);
    await taken(page, dir);
    const said = `${toasts}.includes(${JSON.stringify(why)})`;
    const asked = `document.querySelector('#confirm-message') !== null`;
    await page.waitFor(`${said} || ${asked}`, 20_000);
    if (await page.eval(asked)) {
      await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Open them').click()`);
    }
    await page.waitFor(said, 20_000);
    assert.match(await page.eval(toasts), /intruder proposed changes that could not be opened/);
    assert.equal(await page.eval(`[...document.querySelectorAll('[role="tab"]')].some(t => t.textContent.includes('Intruder'))`), false);
  }
  assert.equal(names(), ORIGINAL);
  assert.equal(mongosh("replica", `print(db.getCollectionNames().includes("${C}"))`).split("\n").at(-1), "true");

  // A valid one with a hostile value: a string to write, which the user sees first.
  const hostile = `"); db.dropDatabase(); //`;
  tamper(dir, { ...base, updates: [{ key: [["_id", B]], set: [["name", hostile]] }] }, "Hostile value");
  await opened(page, "Hostile value");
  await page.waitFor(`${editBar}.includes('1 row changed')`, 30_000);
  assert.equal(await preview(page), `db.${C}.updateOne({ _id: ObjectId("${B}") }, { $set: { name: ${JSON.stringify(hostile)} } })`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  assert.equal(names(), ORIGINAL);
}

export default async function (page, { dir, shots }) {
  const mcp = await startMcp(dir, "e2e-agent");
  try {
    await threeKinds(page, mcp, shots);
    await numbersAndDiscard(page, mcp);
    await partlyStale(page, mcp);
    await refusals(mcp);
    await tampered(page, dir);
  } finally {
    mcp.close();
  }
}
