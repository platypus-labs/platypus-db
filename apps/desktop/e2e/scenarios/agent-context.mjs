// Agents, user context on consent (group N): an agent asks for what the user looks at with
// get_user_context, and gets it only if the user allows it in the open app. The dialog names
// the active tab and connection and lists what would be shared (the editor's text, the
// selection, the result's rows and columns). Share once returns the text, the selection and
// the first 100 rows of the grid (NULL as null, truncated says there are more); refusing
// returns the reason typed. Share for this session answers the next requests of that agent
// without a dialog, reading the editor and the grid again, while another agent is asked. A
// tab on a connection closed to agents is not returned, even once the session is allowed.
import assert from "node:assert/strict";
import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, SHIFT } from "../cdp.mjs";
import { noMcp, startMcp } from "../mcp.mjs";
import { setField } from "../plans.mjs";
import { runQuery, treeRow } from "../query.mjs";

const SHOP = "00000000-0000-4000-8000-000000000051";
const SECRET = "00000000-0000-4000-8000-000000000052";

const dialog = `document.querySelector('.agent-context')`;
const field = (name) => `(${dialog}?.querySelector('[data-field=${JSON.stringify(name)}]')?.textContent ?? '')`;
const item = (part) => `(${dialog}?.querySelector('[data-part=${JSON.stringify(part)}]')?.textContent ?? '')`;
const answer = (verdict) => `${dialog}.querySelector('[data-answer=${JSON.stringify(verdict)}]').click()`;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;

export function skip() {
  return noMcp();
}

export function setup({ dir }) {
  const path = join(dir, "context.db");
  const db = new DatabaseSync(path);
  // 150 orders, the second without a note: more rows than an agent gets, and a NULL.
  db.exec(`CREATE TABLE orders (id INTEGER PRIMARY KEY, note TEXT);
    WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 150)
    INSERT INTO orders SELECT i, CASE WHEN i = 2 THEN NULL ELSE 'note ' || i END FROM n;`);
  db.close();
  const connection = (id, name, access) => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlite"
database = '${path}'
agent_access = "${access}"
`;
  writeFileSync(join(dir, "connections.toml"), `version = 4\n${connection(SHOP, "Shop", "read_only")}${connection(SECRET, "Secret", "off")}`);
}

/** Opens a SQL tab on the connection `name` by a double click on its row. */
async function openTab(page, name) {
  const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${before} && ${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await page.eval(`document.getElementById('e2e-conn')?.removeAttribute('id')`);
}

/** Selects the first `n` characters of the visible editor with the keyboard. */
async function selectStart(page, n) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  // A completion list left open by the typing ignores keys for a moment.
  await page.key("Escape", "Escape", 27);
  await page.sleep(200);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < n; i++) await page.key("ArrowRight", "ArrowRight", 39, SHIFT);
}

/** Calls get_user_context and waits for the app's dialog. */
async function asked(page, mcp, args = {}) {
  const result = mcp.call("get_user_context", args);
  await page.waitFor(`${dialog} !== null`, 20_000);
  return result;
}

/** The bridge's file, once the app wrote it. */
async function bridgeReady(page, dir) {
  const file = join(dir, "agent-bridge.json");
  for (let i = 0; i < 100 && !existsSync(file); i++) await page.sleep(100);
  assert.ok(existsSync(file), "the app wrote agent-bridge.json");
}

const SQL = "SELECT id, note FROM orders ORDER BY id";

/** The dialog lists what would be shared; share once returns it, within the limits. */
async function shareOnce(page, mcp) {
  const result = await asked(page, mcp);
  assert.match(await page.eval(`${dialog}.querySelector('h2').textContent`), /e2e-context wants to see what you are working on/);
  assert.match(await page.eval(field("tab")), /Shop/);
  assert.match(await page.eval(item("editor")), /The editor's text: 39 characters/);
  assert.match(await page.eval(item("selection")), /The selection: 6 characters/);
  assert.match(await page.eval(item("result")), /The result: 150 rows, 2 columns/);
  assert.match(await page.eval(item("result")), /the agent gets the first 100/);
  await page.eval(answer("once"));
  const r = await result;
  assert.equal(r.error, false, r.text);
  await page.waitFor(`${dialog} === null`);
  const context = JSON.parse(r.text);
  assert.equal(context.connection, "Shop");
  assert.equal(context.connection_id, SHOP);
  assert.equal(context.editor, SQL);
  assert.equal(context.selection, "SELECT");
  assert.deepEqual(context.result.columns, ["id", "note"]);
  assert.equal(context.result.rows.length, 100);
  assert.deepEqual(context.result.rows[0], ["1", "note 1"]);
  assert.deepEqual(context.result.rows[1], ["2", null]);
  assert.equal(context.result.truncated, true);
}

/** A refusal carries the reason typed; asking only for some parts shows only those. */
async function refuse(page, mcp) {
  const result = await asked(page, mcp, { parts: ["selection"] });
  assert.equal(await page.eval(`${dialog}.querySelector('[data-part=editor]') === null`), true, "only what is asked is listed");
  assert.match(await page.eval(item("selection")), /The selection: 6 characters/);
  await setField(page, ".agent-context [data-field=reason]", "not this one");
  await page.eval(answer("deny"));
  const r = await result;
  assert.ok(r.error && /did not share what they are working on: not this one/.test(r.text), r.text);
  await page.waitFor(`${dialog} === null`);
  // The reason typed is gone for the next request.
  const next = await asked(page, mcp, { parts: ["editor"] });
  assert.equal(await page.eval(`${dialog}.querySelector('[data-field=reason]').value`), "");
  await page.eval(answer("deny"));
  const denied = await next;
  assert.ok(denied.error && /did not share what they are working on$/.test(denied.text), denied.text);
  await page.waitFor(`${dialog} === null`);
}

/** Allowed for the session: the next requests of this agent are answered without a dialog,
 *  with the editor and the grid as they are then; another agent is asked. */
async function shareSession(page, dir, mcp) {
  const first = await asked(page, mcp, { parts: ["editor", "selection"] });
  await page.eval(answer("session"));
  const r = await first;
  assert.equal(r.error, false, r.text);
  assert.equal(JSON.parse(r.text).selection, "SELECT");
  await page.waitFor(`${dialog} === null`);

  // The user goes on: another statement, with another result.
  const sql = "SELECT id FROM orders WHERE id < 4 ORDER BY id";
  await runQuery(page, sql);
  const again = await mcp.call("get_user_context");
  assert.equal(again.error, false, again.text);
  assert.equal(await page.eval(`${dialog} === null`), true, "not asked again");
  const context = JSON.parse(again.text);
  assert.equal(context.editor, sql);
  assert.equal(context.selection, null, "nothing selected now");
  assert.deepEqual(context.result.columns, ["id"]);
  assert.deepEqual(context.result.rows, [["1"], ["2"], ["3"]]);
  assert.equal(context.result.truncated, false);

  // Another agent has its own answer to give.
  const other = await startMcp(dir, "e2e-other");
  try {
    const result = await asked(page, other);
    assert.match(await page.eval(`${dialog}.querySelector('h2').textContent`), /e2e-other wants to see/);
    await page.eval(answer("deny"));
    assert.equal((await result).error, true);
    await page.waitFor(`${dialog} === null`);
  } finally {
    other.close();
  }
}

/** A tab on a connection closed to agents is not returned, though the session is allowed. */
async function closedConnection(page, mcp) {
  await openTab(page, "Secret");
  await runQuery(page, "SELECT 'classified' AS secret");
  const r = await mcp.call("get_user_context");
  assert.ok(r.error && /not open to agents/.test(r.text), r.text);
  assert.ok(!r.text.includes("classified"), r.text);
  assert.equal(await page.eval(`${dialog} === null`), true);
}

export default async function (page, { dir }) {
  await bridgeReady(page, dir);
  await openTab(page, "Shop");
  await runQuery(page, SQL);
  await selectStart(page, 6);

  const mcp = await startMcp(dir, "e2e-context");
  try {
    await shareOnce(page, mcp);
    await refuse(page, mcp);
    await shareSession(page, dir, mcp);
    await closedConnection(page, mcp);
  } finally {
    mcp.close();
  }
}
