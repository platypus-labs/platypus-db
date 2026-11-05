// What masks for agents is sealed like the rest of what they may do. An agent that edits
// settings.toml or catalog.db to lift a rule or a mark reads the values masked all the same, until
// the user accepts the edit in the app; an addition applies at once; what the user changes in the
// app applies at once too. A seal that no longer matches masks every value until the user answers.
import assert from "node:assert/strict";
import { appendFileSync, existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { noMcp, startMcp } from "../mcp.mjs";

const PEOPLE = "00000000-0000-4000-8000-0000000000d1";
const EMAIL = "ann@example.com";
const MASKED = "••••••";
const SELECT = "SELECT id, name, email, nick FROM people ORDER BY id";

const changesBox = `document.querySelector('[data-agent-policy-changes]')`;
const question = `document.querySelector('.modal.confirm')`;
const key = (column) => ({ database: "", schema: "main", object: "people", column });

export const skip = () => noMcp();

const settingsText = (names, types = "") => `version = 1\n\n[safety]\nmask_names = "${names}"\nmask_types = "${types}"\n`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "people.db"));
  db.exec(`CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, email TEXT, nick TEXT, note TEXT);
           INSERT INTO people VALUES (1, 'Ann', '${EMAIL}', 'annie', 'n1'), (2, 'Bob', NULL, 'bo', 'n2');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n\n[[connections]]\nid = "${PEOPLE}"\nname = "People"\ndriver = "sqlite"\ndatabase = '${join(dir, "people.db")}'\nsave_password = false\n`,
  );
  writeFileSync(join(dir, "settings.toml"), settingsText("*email*, nick", "inet"));
}

/** Waits until `check()` holds: the app writes the seal a moment after it looks. */
async function eventually(check, what) {
  for (let i = 0; i < 150; i++) {
    if (check()) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  assert.fail(`never: ${what}`);
}

/** The rows of the statement on People, as an agent reads them. */
async function read(mcp, sql = SELECT) {
  const r = await mcp.call("run_query", { connection: "People", sql, format: "json" });
  assert.equal(r.error, false, r.text);
  return JSON.parse(r.text);
}

const invoke = (page, command, args = {}) =>
  page.eval(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`);

/** What the app says is waiting for the user, when it reads everything. */
const pending = (page) => invoke(page, "agent_policy_changes", { marks: true });

/** The marks in catalog.db, by column. */
function marksInFile(dir) {
  const db = new DatabaseSync(join(dir, "catalog.db"));
  const rows = db.prepare("SELECT column_name, sensitive FROM sensitive_columns").all();
  db.close();
  return Object.fromEntries(rows.map((r) => [r.column_name, Number(r.sensitive)]));
}

/** What an agent with file tools does: takes rules and marks out of the files. */
function liftByHand(dir) {
  writeFileSync(join(dir, "settings.toml"), settingsText("nick"));
  const db = new DatabaseSync(join(dir, "catalog.db"));
  db.exec("PRAGMA busy_timeout = 5000");
  db.exec("DELETE FROM sensitive_columns WHERE column_name = 'name'");
  db.exec(
    `INSERT OR REPLACE INTO sensitive_columns VALUES ('${PEOPLE}', '', 'main', 'people', 'email', 0)`,
  );
  db.close();
}

/** Dismisses the question if the window's focus brought it up: Security lists the same changes. */
async function dismissQuestion(page) {
  await page.eval(`document.querySelector('.modal.confirm .actions button')?.click()`);
}

async function openSecurity(page) {
  await page.waitFor(`document.querySelector('.sidebar-header') !== null`);
  await dismissQuestion(page);
  if (!(await page.eval(`document.querySelector('.modal.settings') !== null`))) {
    await page.key(",", "Comma", 188, CTRL);
    await page.waitFor(`document.querySelector('.modal.settings') !== null`, 10_000);
  }
  await page.eval(`document.querySelector('.modal.settings [role="tab"][data-section="Agents"]').click()`);
  await page.waitFor(`document.querySelector('[data-agents-page="security"]') !== null`, 10_000);
  await page.eval(`document.querySelector('[data-agents-page="security"]').click()`);
}

/** Dispatches the window's focus: the app looks at the files and asks. */
const focus = (page) => page.eval(`window.dispatchEvent(new Event("focus"))`);

export default async function (page, { dir }) {
  const mcp = await startMcp(dir, "e2e-sealed-masking");
  const settings = () => readFileSync(join(dir, "settings.toml"), "utf8");
  const policy = join(dir, "agent-policy.json");
  const marksFile = join(dir, "agent-policy-marks.json");
  const sealedRules = () => {
    try {
      return JSON.parse(readFileSync(policy, "utf8")).masking ?? null;
    } catch {
      return null;
    }
  };
  try {
    // The first look seals the rules as they are, without asking. The marks are compared a moment
    // after the start: past it, what follows finds no check on its way.
    await eventually(() => sealedRules() !== null, "the rules in the first seal");
    assert.match(sealedRules().names, /\*email\*/);
    assert.deepEqual(await pending(page), []);
    await page.sleep(5_500);

    // A mark set in the app is sealed at once, and compactly in its own file.
    await invoke(page, "set_catalog_sensitive", { connectionId: PEOPLE, key: key("name"), sensitive: true });
    assert.ok(existsSync(marksFile), "the marks file");
    assert.match(readFileSync(marksFile, "utf8"), /"y":\["name"\]/);
    const start = await read(mcp);
    assert.deepEqual(start.rows, [[1, MASKED, MASKED, MASKED], [2, MASKED, null, MASKED]]);

    // A rule added by hand masks at once, with nothing to ask.
    writeFileSync(join(dir, "settings.toml"), settingsText("*email*, nick, *note*", "inet"));
    const added = await read(mcp, "SELECT id, note FROM people ORDER BY id");
    assert.deepEqual(added.rows, [[1, MASKED], [2, MASKED]]);
    assert.deepEqual(added.masked_columns, ["note"]);

    // Rules and marks taken out by hand, and a mark "not sensitive" added: nothing comes through.
    liftByHand(dir);
    const lifted = await read(mcp);
    assert.deepEqual(lifted.rows, [[1, MASKED, MASKED, MASKED], [2, MASKED, null, MASKED]], "edits outside the app wait for the user");
    for (const sql of ["SELECT email AS contact FROM people", "SELECT upper(email) FROM people", "SELECT * FROM people"]) {
      const text = (await mcp.call("run_query", { connection: "People", sql })).text;
      assert.ok(!text.toLowerCase().includes(EMAIL), `${sql}: ${text}`);
    }
    const sample = await mcp.call("sample_rows", { connection: "People", schema: "main", table: "people" });
    assert.ok(!sample.text.includes(EMAIL) && !sample.text.includes("Ann"), sample.text);

    // The question of the app names what was lifted. Undo puts the files back.
    await focus(page);
    await page.waitFor(`${question} !== null`, 15_000);
    const asked = await page.eval(`${question}.textContent`);
    assert.match(asked, /\*email\*/);
    assert.match(asked, /inet/);
    assert.match(asked, /main\.people\.name/);
    assert.match(asked, /main\.people\.email/);
    await page.eval(`${question}.querySelector('button.primary').click()`);
    await page.waitFor(`${question} === null`);
    await eventually(() => /\*email\*/.test(settings()) && /inet/.test(settings()), "the rules back in settings.toml");
    assert.deepEqual(marksInFile(dir), { name: 1 }, "the marks back in catalog.db");
    assert.deepEqual(await pending(page), []);
    assert.deepEqual((await read(mcp)).rows, [[1, MASKED, MASKED, MASKED], [2, MASKED, null, MASKED]]);

    // Again, and this time accepted in Settings > Agents > Security: the values show.
    liftByHand(dir);
    assert.deepEqual((await read(mcp)).rows[0], [1, MASKED, MASKED, MASKED], "still waiting");
    await openSecurity(page);
    await page.waitFor(`${changesBox} !== null`, 15_000);
    const listed = await page.eval(`${changesBox}.textContent`);
    assert.match(listed, /\*email\*/);
    assert.match(listed, /main\.people\.name/);
    await dismissQuestion(page);
    // A yes holds for what the question said: for anything else, here a question that listed
    // less than the files lift, nothing is accepted and the values stay masked.
    const stale = await page.eval(
      `window.__TAURI_INTERNALS__.invoke("answer_agent_policy", { accept: true, changes: [] }).then(() => "accepted", (e) => e.kind ?? String(e))`,
    );
    assert.equal(stale, "changed");
    assert.deepEqual((await read(mcp)).rows[0], [1, MASKED, MASKED, MASKED], "nothing was accepted");
    await page.eval(`${changesBox}.querySelector('[data-agent-policy="accept"]').click()`);
    await page.waitFor(`${changesBox} === null`, 15_000);
    await dismissQuestion(page);
    const accepted = await read(mcp);
    assert.deepEqual(accepted.rows, [[1, "Ann", EMAIL, MASKED], [2, "Bob", null, MASKED]], "nick is still in the file's rules");
    assert.deepEqual(await pending(page), []);

    // What the user changes in the app needs no question, and applies at once, either way.
    await invoke(page, "set_setting", { key: "safety.mask_names", value: "" });
    assert.deepEqual((await read(mcp)).rows[0], [1, "Ann", EMAIL, "annie"]);
    await invoke(page, "set_catalog_sensitive", { connectionId: PEOPLE, key: key("name"), sensitive: true });
    assert.deepEqual((await read(mcp)).rows[0], [1, MASKED, EMAIL, "annie"]);
    await invoke(page, "set_catalog_sensitive", { connectionId: PEOPLE, key: key("name"), sensitive: null });
    assert.deepEqual((await read(mcp)).rows[0], [1, "Ann", EMAIL, "annie"]);
    assert.deepEqual(await pending(page), []);

    // A policy file that no longer matches its seal: what was approved cannot be told, so every
    // value masks until the user answers. It is not "nothing is masked".
    appendFileSync(policy, " ");
    assert.deepEqual((await read(mcp)).rows[0], [MASKED, MASKED, MASKED, MASKED]);
    await focus(page);
    await page.waitFor(`${changesBox} !== null`, 15_000);
    assert.ok((await pending(page)).some((c) => c.kind === "masking_lost"), "the question says the seal no longer matches");
    await dismissQuestion(page);
    await page.eval(`${changesBox}.querySelector('[data-agent-policy="accept"]').click()`);
    await page.waitFor(`${changesBox} === null`, 15_000);
    await dismissQuestion(page);
    assert.deepEqual((await read(mcp)).rows[0], [1, "Ann", EMAIL, "annie"]);

    // The marks file gone: a process that has read it keeps what it read, a new one masks all.
    await invoke(page, "set_catalog_sensitive", { connectionId: PEOPLE, key: key("name"), sensitive: true });
    assert.deepEqual((await read(mcp)).rows[0], [1, MASKED, EMAIL, "annie"]);
    rmSync(marksFile);
    assert.deepEqual((await read(mcp)).rows[0], [1, MASKED, EMAIL, "annie"]);
    const fresh = await startMcp(dir, "e2e-sealed-masking-new");
    try {
      assert.deepEqual((await read(fresh)).rows[0], [MASKED, MASKED, MASKED, MASKED]);
      // Undone: the defaults and what the files add, since nothing else can be told.
      await focus(page);
      await page.waitFor(`${changesBox} !== null`, 15_000);
      await dismissQuestion(page);
      await page.eval(`${changesBox}.querySelector('[data-agent-policy="undo"]').click()`);
      await page.waitFor(`${changesBox} === null`, 15_000);
      await dismissQuestion(page);
      assert.deepEqual((await read(fresh)).rows[0], [1, MASKED, MASKED, "annie"]);
      assert.match(settings(), /\*email\*/);
    } finally {
      fresh.close();
    }
  } finally {
    mcp.close();
  }
}
