// A connection's limits for agents: set in Settings > Agents > Security, they lower what an agent
// asks for (and the result says so), count its calls per minute, and let a read through with a
// note where the database gives no estimate for the cost guard. They only tighten: a value
// written by hand to raise or remove a limit gets the agent nothing, a lower one applies at once,
// and a number that is not a positive whole number is refused by the field.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { noMcp, startMcp } from "../mcp.mjs";

const PEOPLE = "00000000-0000-4000-8000-0000000000f1";
const OTHER = "00000000-0000-4000-8000-0000000000f2";

const row = (name) => `document.querySelector('.agents-panel tr[data-connection=${JSON.stringify(name)}]')`;
const field = (key) => `document.querySelector('.agent-detail [data-limit="${key}"]')`;

export const skip = () => noMcp();

export function setup({ dir }) {
  const entry = (id, name, file) =>
    `\n[[connections]]\nid = "${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${join(dir, file)}'\nsave_password = false\n`;
  for (const file of ["people.db", "other.db"]) {
    const db = new DatabaseSync(join(dir, file));
    db.exec("CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT);");
    for (let i = 1; i <= 30; i++) db.exec(`INSERT INTO people VALUES (${i}, 'p${i}')`);
    db.close();
  }
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${entry(PEOPLE, "People", "people.db")}${entry(OTHER, "Other", "other.db")}`);
}

async function chooseInSecurity(page, name) {
  await page.waitFor(`document.querySelector('.sidebar-header') !== null`);
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`, 10_000);
  await page.eval(`document.querySelector('.modal.settings [role="tab"][data-section="Agents"]').click()`);
  await page.waitFor(`document.querySelector('[data-agents-page="security"]') !== null`, 10_000);
  await page.eval(`document.querySelector('[data-agents-page="security"]').click()`);
  await page.waitFor(`${row(name)} !== null`);
  await page.eval(`${row(name)}.click()`);
  await page.waitFor(`${field("max_rows")} !== null`);
}

/** Replaces the text of a limit field and presses Enter. */
async function enter(page, key, text) {
  await page.eval(`${field(key)}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  if (text === "") await page.key("Backspace", "Backspace", 8);
  else await page.type(text);
  await page.waitFor(`${field(key)}.value === ${JSON.stringify(text)}`);
  await page.key("Enter", "Enter", 13);
}

/** Waits until `check()` holds: the app writes a field to the file a moment after Enter. */
async function eventually(check, what) {
  for (let i = 0; i < 100; i++) {
    if (check()) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  assert.fail(`never: ${what}`);
}

/** The JSON result of a run_query on `name`. */
async function read(mcp, name, args = {}) {
  const r = await mcp.call("run_query", { connection: name, sql: "SELECT id FROM people ORDER BY id", format: "json", ...args });
  assert.equal(r.error, false, r.text);
  return JSON.parse(r.text);
}

export default async function (page, { dir }) {
  const file = join(dir, "connections.toml");
  const connections = () => readFileSync(file, "utf8");
  const mcp = await startMcp(dir, "e2e-limits");
  try {
    // No limit: the agent gets what it asks for, and the result has no notes.
    const free = await read(mcp, "People", { max_rows: 20 });
    assert.equal(free.row_count, 20);
    assert.equal(free.notes, undefined);

    // Rows per result, set in the app: the fixed cap is the placeholder.
    await chooseInSecurity(page, "People");
    assert.equal(await page.eval(`${field("max_rows")}.placeholder`), "10000");
    assert.equal(await page.eval(`${field("calls_per_minute")}.value`), "");
    await enter(page, "max_rows", "5");
    await eventually(() => /\[connections\.agent_limits\]\s+max_rows = 5/.test(connections()), "max_rows in the file");
    const lowered = await read(mcp, "People", { max_rows: 20 });
    assert.equal(lowered.row_count, 5);
    assert.equal(lowered.truncated_by, "max_rows");
    assert.match(lowered.notes[0], /max_rows was lowered to 5/);
    // Asking for less, or for nothing, says nothing; the other connection is not touched.
    assert.equal((await read(mcp, "People", { max_rows: 3 })).notes, undefined);
    assert.equal((await read(mcp, "People")).row_count, 5);
    assert.equal((await read(mcp, "Other", { max_rows: 20 })).row_count, 20);
    const sample = await mcp.call("sample_rows", { connection: "People", schema: "main", table: "people" });
    assert.match(sample.text, /\(5 rows/);

    // What is not a positive whole number is refused by the field and never saved.
    for (const bad of ["abc", "0", "-3", "1.5", "1e3"]) {
      await enter(page, "max_bytes", bad);
      await page.waitFor(`${field("max_bytes")}.getAttribute("aria-invalid") === "true"`);
      await page.sleep(300);
      assert.doesNotMatch(connections(), /max_bytes/, bad);
    }
    await enter(page, "max_bytes", "");
    await page.waitFor(`${field("max_bytes")}.getAttribute("aria-invalid") === "false"`);

    // A raise written by hand waits for the user: the agent keeps the limit it had.
    const approved = connections();
    writeFileSync(file, approved.replace("max_rows = 5", "max_rows = 500"));
    assert.equal((await read(mcp, "People", { max_rows: 20 })).row_count, 5, "a raised limit waits");
    // So does removing it.
    writeFileSync(file, approved.replace(/\n\[connections\.agent_limits\]\s+max_rows = 5\n/, "\n"));
    assert.equal((await read(mcp, "People", { max_rows: 20 })).row_count, 5, "a removed limit waits");
    writeFileSync(file, approved);
    // A lower one applies at once.
    writeFileSync(file, approved.replace("max_rows = 5", "max_rows = 2"));
    assert.equal((await read(mcp, "People", { max_rows: 20 })).row_count, 2, "a tighter limit applies");

    // The user raises it in the app: that is an approval.
    await enter(page, "max_rows", "8");
    await eventually(() => /max_rows = 8/.test(connections()), "max_rows = 8 in the file");
    assert.equal((await read(mcp, "People", { max_rows: 20 })).row_count, 8);

    // Estimated cost: SQLite gives no estimate, so the read runs and says it was not checked.
    await enter(page, "max_estimated_rows", "1");
    await eventually(() => /max_estimated_rows = 1\b/.test(connections()), "max_estimated_rows in the file");
    const unchecked = await mcp.call("run_query", { connection: "People", sql: "SELECT id FROM people ORDER BY id" });
    assert.equal(unchecked.error, false, unchecked.text);
    assert.match(unchecked.text, /could not be checked/);
    const quiet = await mcp.call("run_query", { connection: "Other", sql: "SELECT id FROM people ORDER BY id" });
    assert.doesNotMatch(quiet.text, /checked/);

    // Calls per minute count run_query, sample_rows and explain together, per connection and
    // per server process: a new process starts with an empty window.
    await enter(page, "calls_per_minute", "3");
    await eventually(() => /calls_per_minute = 3/.test(connections()), "calls_per_minute in the file");
    const counting = await startMcp(dir, "e2e-limits-calls");
    try {
      const sql = "SELECT 1";
      assert.equal((await counting.call("run_query", { connection: "People", sql })).error, false);
      assert.equal((await counting.call("sample_rows", { connection: "People", schema: "main", table: "people" })).error, false);
      assert.equal((await counting.call("explain", { connection: "People", sql })).error, false);
      for (const [tool, args] of [
        ["run_query", { sql }],
        ["explain", { sql }],
        ["sample_rows", { schema: "main", table: "people" }],
      ]) {
        const refused = await counting.call(tool, { connection: "People", ...args });
        assert.equal(refused.error, true, `${tool}: ${refused.text}`);
        assert.match(refused.text, /3 calls a minute.*try again in \d+ seconds/);
      }
      assert.equal((await counting.call("list_schemas", { connection: "People" })).error, false, "metadata is not counted");
      assert.equal((await counting.call("run_query", { connection: "Other", sql })).error, false, "another connection has its own window");
    } finally {
      await counting.close?.();
    }
    // The first process counted nothing of that.
    assert.equal((await mcp.call("run_query", { connection: "People", sql: "SELECT 1" })).error, false);

    // Emptying every field takes the limits out of the file.
    for (const key of ["max_rows", "max_estimated_rows", "calls_per_minute"]) {
      await enter(page, key, "");
      // One at a time: each save starts from the connection as the last one left it.
      await eventually(() => !connections().includes(`${key} =`), `${key} removed from the file`);
    }
    await eventually(() => !/agent_limits/.test(connections()), "limits removed from the file");
    assert.equal((await read(mcp, "People", { max_rows: 20 })).row_count, 20);
  } finally {
    await mcp.close?.();
  }
}
