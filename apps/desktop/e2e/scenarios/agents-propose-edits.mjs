// Rows an agent proposes with propose_edits open in the table's data viewer of the open app as
// ordinary pending edits (new, changed and deleted rows), and change nothing until the user
// saves: Save writes them, Discard and Undo leave the table as it was; rows that cannot be
// applied are said so while the rest is; a tampered row in the profile's file, with hostile
// text in a table name, a column name or a key value, runs nothing and leaves the table intact;
// a table with no key is refused; a second proposal on the same table has its own tab and
// changes the first one's edits not at all.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";

const ID = "00000000-0000-4000-8000-0000000000f1";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const notice = `(${visibleTab}?.querySelector('.edit-notice')?.textContent ?? '')`;
const button = (label) => `${visibleTab}.querySelector('.edit-bar button[aria-label="${label}"]')`;
const tabTitled = (title) =>
  `[...document.querySelectorAll('[role="tab"]')].some(t => t.textContent.includes(${JSON.stringify(title)}))`;
const waitingSign = `(${visibleTab}?.querySelector('.edit-waiting')?.textContent ?? '')`;
const toasts = `[...document.querySelectorAll('.toaster .toast')].map(t => t.textContent).join(' | ')`;

export const skip = () => noMcp();

export function setup({ dir }) {
  const path = join(dir, "edits.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, city TEXT, age INTEGER);
           INSERT INTO people VALUES (1, 'Ada', 'London', 36), (2, 'Bob', 'Paris', 41), (3, 'Cy', 'Rome', 29);
           CREATE TABLE loose (v TEXT);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "${ID}"
name = "Edits"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

/** The table's rows as they are in the file, read on a connection of its own. */
function people(dir) {
  const db = new DatabaseSync(join(dir, "edits.db"));
  try {
    return db.prepare("SELECT id, name, city, age FROM people ORDER BY id").all().map((r) => ({ ...r }));
  } finally {
    db.close();
  }
}

const ORIGINAL = [
  { id: 1, name: "Ada", city: "London", age: 36 },
  { id: 2, name: "Bob", city: "Paris", age: 41 },
  { id: 3, name: "Cy", city: "Rome", age: 29 },
];

/** Waits for the file to hold `rows`: a save finishes a moment after its button is clicked. */
async function fileHolds(page, dir, rows) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (JSON.stringify(people(dir)) === JSON.stringify(rows)) return;
    await page.sleep(200);
  }
  assert.deepEqual(people(dir), rows);
}

/** Records every text the "applying" sign shows from now on, whatever tab it is on, so a sign that
 *  comes and goes between two polls is still seen. Cleared first: what an earlier proposal showed
 *  is not this one's. */
async function watchSign(page) {
  await page.eval(`(() => {
    window.__signs = [];
    window.__signObserver?.disconnect();
    window.__signObserver = new MutationObserver(() => {
      for (const e of document.querySelectorAll('.edit-waiting')) {
        if (!window.__signs.includes(e.textContent)) window.__signs.push(e.textContent);
      }
    });
    window.__signObserver.observe(document.body, { subtree: true, childList: true, characterData: true });
  })()`);
}

/** Proposes through the MCP server and waits for the tab the app opens for it. */
async function propose(page, mcp, args, title) {
  const r = await mcp.call("propose_edits", { connection: "Edits", schema: "main", table: "people", title, ...args });
  assert.equal(r.error, false, r.text);
  assert.match(r.text, /Nothing changed/);
  await opened(page, title);
  return r.text;
}

/** Waits for the tab titled `title`. More than a few proposals in a minute are asked about
 *  first, as for SQL: the question is answered yes. */
async function opened(page, title) {
  const asked = `document.querySelector('#confirm-message') !== null`;
  await page.waitFor(`${tabTitled(title)} || ${asked}`, 30_000);
  if (await page.eval(asked)) {
    assert.match(await page.eval(`document.querySelector('#confirm-message').textContent`), /proposed 1 change to table data/);
    await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Open them').click()`);
  }
  await page.waitFor(tabTitled(title), 30_000);
}

/** Writes a proposal into the profile's file as a program other than the app would. */
function tamper(dir, edits, title, agent = "intruder") {
  const db = new DatabaseSync(join(dir, "agent-calls.db"));
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    db.prepare(
      "INSERT INTO proposals (agent, connection_id, title, sql, edits, created_at) VALUES (?, ?, ?, '', ?, ?)",
    ).run(agent, ID, title, typeof edits === "string" ? edits : JSON.stringify(edits), Date.now());
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

async function threeKinds(page, dir, mcp, shots) {
  await watchSign(page);
  const text = await propose(
    page,
    mcp,
    {
      inserts: [{ name: "Dee", city: "Oslo", age: 33 }],
      updates: [{ key: { id: 1 }, set: { city: "Berlin" } }],
      deletes: [{ id: 3 }],
    },
    "Tidy people",
  );
  assert.match(text, /1 rows to insert, 1 to update, 1 to delete/);
  // The three kinds are pending in the grid, the table is as it was.
  await page.waitFor(`${editBar}.includes('3 rows changed (1 modified, 1 new, 1 deleted)')`, 30_000);
  assert.equal(await page.eval(`${notice}`), "", "nothing was left out");
  // The sign was there while the proposal waited, named its agent, and is gone once applied.
  const signs = await page.eval(`window.__signs`);
  assert.equal(signs.length, 1, JSON.stringify(signs));
  assert.equal(signs[0], "Applying the changes proposed by e2e-agent…");
  assert.equal(await page.eval(waitingSign), "", "the sign goes with the wait");
  assert.deepEqual(people(dir), ORIGINAL);
  assert.ok(await page.eval(`${visibleTab}.querySelector('.statusbar span') !== null`));
  if (shots) await page.screenshot(join(shots, "propose-edits-pending.png"));

  // The SQL that Save would run is the ordinary one.
  await page.eval(`${button("Preview SQL")}.click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  assert.equal(
    await page.eval(`document.querySelector('.sql-preview').textContent`),
    "DELETE FROM main.people WHERE id = 3;\n" +
      "UPDATE main.people SET city = 'Berlin' WHERE id = 1;\n" +
      "INSERT INTO main.people (name, city, age) VALUES ('Dee', 'Oslo', 33);",
  );
  assert.deepEqual(people(dir), ORIGINAL, "previewing writes nothing");
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  await fileHolds(page, dir, [
    { id: 1, name: "Ada", city: "Berlin", age: 36 },
    { id: 2, name: "Bob", city: "Paris", age: 41 },
    { id: 3, name: "Dee", city: "Oslo", age: 33 },
  ]);
  // The viewer reads the rows again, and the proposal is not applied a second time.
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await page.sleep(1500);
  assert.match(await page.eval(editBar), /to edit a cell/);
}

async function discardAndUndo(page, dir, mcp) {
  await propose(
    page,
    mcp,
    { updates: [{ key: { id: 2 }, set: { city: "Madrid" } }], deletes: [{ id: 3 }] },
    "Discard these",
  );
  await page.waitFor(`${editBar}.includes('2 rows changed (1 modified, 1 deleted)')`, 30_000);
  await page.eval(`${button("Discard")}.click()`);
  await page.waitFor(`${editBar}.includes('to edit a cell')`);
  await page.sleep(500);
  assert.equal(people(dir)[1].city, "Paris");
  assert.equal(people(dir).length, 3);

  // Undo takes the applied proposal back like any edit of the user's.
  await propose(page, mcp, { updates: [{ key: { id: 2 }, set: { name: "Bobby" } }] }, "Undo this");
  await page.waitFor(`${editBar}.includes('1 row changed')`, 30_000);
  await page.eval(`${button("Undo")}.click()`);
  await page.waitFor(`${editBar}.includes('to edit a cell')`);
  assert.equal(people(dir)[1].name, "Bob");
}

async function whatCannotBeApplied(page, dir, mcp) {
  // Id 7 never was, nor id 99; 'abc' is no age.
  await propose(
    page,
    mcp,
    {
      updates: [
        { key: { id: 2 }, set: { city: "Lyon" } },
        { key: { id: 1 }, set: { age: "abc" } },
        { key: { id: 99 }, set: { city: "Nowhere" } },
      ],
      deletes: [{ id: 7 }],
    },
    "Partly stale",
  );
  await page.waitFor(`${notice}.includes('3 proposed rows could not be applied')`, 30_000);
  // The result notice takes the sign's place: the two are never shown together.
  assert.equal(await page.eval(waitingSign), "");
  const said = await page.eval(notice);
  assert.match(said, /no row of the table has that key/);
  assert.match(said, /the value for age does not fit its type/);
  assert.match(said, /Update 3/);
  assert.match(said, /Delete 1/);
  // The row that could be applied is pending; nothing was written.
  assert.match(await page.eval(editBar), /1 row changed/);
  assert.deepEqual(people(dir).map((r) => r.city), ["Berlin", "Paris", "Oslo"]);

  // A second proposal on the same table has a tab of its own, and the first keeps its edits.
  const first = `[...document.querySelectorAll('.query-tab')].filter(t => t.querySelector('.edit-notice'))[0]`;
  await propose(page, mcp, { inserts: [{ name: "Eve" }, { name: "Fay" }] }, "Two more");
  await page.waitFor(`${editBar}.includes('2 rows changed')`, 30_000);
  assert.equal(await page.eval(`${notice}`), "");
  assert.match(await page.eval(`${first}.querySelector('.edit-bar').textContent`), /1 row changed/);
  assert.match(await page.eval(`${first}.querySelector('.edit-notice').textContent`), /3 proposed rows could not be applied/);
  assert.deepEqual(people(dir).length, 3);
}

async function refusals(page, dir, mcp) {
  const refused = async (args, what) => {
    const r = await mcp.call("propose_edits", { connection: "Edits", schema: "main", ...args });
    assert.equal(r.error, true, r.text);
    assert.match(r.text, what);
  };
  await refused({ table: "loose", inserts: [{ v: "x" }] }, /no primary key or unique key/);
  await refused({ table: "people", deletes: [{ name: "Ada" }] }, /not exactly the columns of a key/);
  await refused({ table: "people", updates: [{ key: { id: null }, set: { name: "x" } }] }, /NULL/);
  await refused({ table: "people", inserts: [{ name: ["a"] }] }, /array/);
  await refused({ table: "people" }, /no changes/);
  await refused({ table: "nope", inserts: [{ v: "x" }] }, /no table or view/);
  assert.deepEqual(people(dir).length, 3);
}

async function tampered(page, dir) {
  const before = people(dir);
  const base = { schema: "main", table: "people", inserts: [], updates: [], deletes: [] };
  const attacks = [
    // A key value that is SQL, in a column of numbers.
    [{ ...base, deletes: [[["id", "1; DROP TABLE people; --"]]] }, "is not a whole number"],
    [{ ...base, deletes: [[["id", "1 OR 1=1"]]] }, "is not a whole number"],
    // Names that are SQL.
    [{ ...base, table: 'people"; DROP TABLE people; --', deletes: [[["id", "1"]]] }, "does not exist"],
    [{ ...base, deletes: [[['id" = 1; DROP TABLE people; --', "1"]]] }, "which the table does not have"],
    // A table with no key, and a key that is not one.
    [{ ...base, table: "loose", inserts: [[["v", "x"]]] }, "no primary key or unique key"],
    [{ ...base, deletes: [[["name", "Ada"]]] }, "not exactly the columns of a key"],
    // Not even a proposal.
    ["{not json", "not a list of rows to change"],
    [{ ...base, deletes: "nope" }, "not a list of rows to change"],
  ];
  for (const [i, [edits, why]] of attacks.entries()) {
    // The previous attack's toast says the same: dismissed, or the wait below would not wait.
    await page.eval(`document.querySelectorAll('.toaster .toast').forEach(t => t.click())`);
    await page.waitFor(`document.querySelector('.toaster .toast') === null`);
    tamper(dir, edits, `Intruder ${i}`);
    // The app took it, refused it with an error that says why, and opened no tab.
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
  assert.deepEqual(people(dir), before);
  const db = new DatabaseSync(join(dir, "edits.db"));
  const tables = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all().map((r) => r.name);
  db.close();
  assert.deepEqual(tables, ["loose", "people"]);

  // One that is valid is applied as pending edits, and still saves nothing by itself: a
  // hostile value is a value to write, and the user sees it in the grid first.
  // The agent's name is text to the sign, never markup.
  const hostile = '<img src=x onerror="window.__pwned = 1"><b>Eve</b>';
  await watchSign(page);
  tamper(
    dir,
    { ...base, updates: [{ key: [["id", "2"]], set: [["name", "x'); DROP TABLE people; --"]] }] },
    "Hostile value",
    hostile,
  );
  await opened(page, "Hostile value");
  await page.waitFor(`${editBar}.includes('1 row changed')`, 30_000);
  assert.deepEqual(await page.eval(`window.__signs`), [`Applying the changes proposed by ${hostile}…`]);
  // The bar's SQL line is a CodeMirror, whose empty `img.cm-widgetBuffer` is its own.
  assert.equal(await page.eval(`${visibleTab}.querySelector('.edit-bar img:not(.cm-widgetBuffer)')?.outerHTML ?? null`), null);
  assert.equal(await page.eval(`[...${visibleTab}.querySelectorAll('.edit-bar b')].some(b => b.textContent === 'Eve')`), false);
  assert.equal(await page.eval(`window.__pwned === undefined`), true);
  await page.eval(`${button("Preview SQL")}.click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`);
  assert.equal(
    await page.eval(`document.querySelector('.sql-preview').textContent`),
    "UPDATE main.people SET name = 'x''); DROP TABLE people; --' WHERE id = 2;",
  );
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  assert.deepEqual(people(dir), before);
}

export default async function (page, { dir, shots }) {
  const mcp = await startMcp(dir, "e2e-agent");
  try {
    await threeKinds(page, dir, mcp, shots);
    await discardAndUndo(page, dir, mcp);
    await whatCannotBeApplied(page, dir, mcp);
    await refusals(page, dir, mcp);
    await tampered(page, dir);
  } finally {
    mcp.close();
  }
}
