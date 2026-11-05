// The agent report in Settings > Agents > Activity: a summary per agent and, under it, per
// session (each start of the MCP server), with calls, tables read, rows returned, changes
// allowed and refused, and errors; it follows the filters of the call list; the list stays
// below it; and it exports as Markdown and as one HTML file next to the CSV. The calls are made
// through the MCP server on SQLite: reads of a table by tool and by statement (one table named
// with a `|`), a change a read-only connection refuses, a change a read-write one allows, and a
// statement that fails. One agent is named like an HTML tag: its name, in the page and in both
// files, must stay text.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { noMcp, startMcp } from "../mcp.mjs";
import { answerSave } from "../qa-xfer.mjs";

const RO = "00000000-0000-4000-8000-0000000000c1";
const RW = "00000000-0000-4000-8000-0000000000c2";
const HOSTILE = "<img src=x onerror=alert(1)>";

const panel = `document.querySelector('[data-agents-panel="activity"]')`;
const report = `document.querySelector('[data-agent-report]')`;
const rowsOf = `[...document.querySelectorAll('[data-agent-report] tbody tr')].map(r => [r.dataset.report, r.dataset.name, ...['calls', 'tables', 'rows', 'allowed', 'refused', 'errors'].map(c => r.querySelector('[data-col=' + c + ']').textContent)])`;
const callTools = `[...document.querySelectorAll('.agent-calls tr[data-call]')].map(r => r.dataset.call)`;

/** Sets a React-controlled input or select and tells React, as typing or choosing would. */
const setValue = (element, value) => `(() => {
  const el = ${element};
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
})()`;

export const skip = () => noMcp();

export function setup({ dir }) {
  const path = join(dir, "shop.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT);
    WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 10)
    INSERT INTO orders SELECT i, 'open' FROM n;
    CREATE TABLE items (order_id INTEGER); INSERT INTO items VALUES (1), (1), (2), (3);
    CREATE TABLE "we|ird" (x INTEGER); INSERT INTO "we|ird" VALUES (1), (2);`);
  db.close();
  const connection = (id, name, access) =>
    `\n[[connections]]\nid = "${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${path}'\nagent_access = "${access}"\nsave_password = false\n`;
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${connection(RO, "Shop", "read_only")}${connection(RW, "Scratch", "read_write")}`);
}

/** The calls of three server runs: the hostile-named agent twice, another agent once. */
async function agentCalls(dir) {
  const first = await startMcp(dir, HOSTILE);
  try {
    let r = await first.call("sample_rows", { connection: "Shop", schema: "main", table: "orders", max_rows: 3 });
    assert.equal(r.error, false, r.text);
    r = await first.call("run_query", { connection: "Shop", sql: 'SELECT * FROM "we|ird"', format: "json" });
    assert.equal(r.error, false, r.text);
    r = await first.call("run_query", { connection: "Shop", sql: "SELECT i.order_id FROM main.items i" });
    assert.equal(r.error, false, r.text);
    // Refused: the connection is read-only for agents.
    r = await first.call("run_query", { connection: "Shop", sql: "DELETE FROM orders WHERE id = 1" });
    assert.equal(r.error, true, r.text);
    // Allowed: a change on the read-write connection.
    r = await first.call("run_query", { connection: "Scratch", sql: "UPDATE orders SET status = 'seen' WHERE id = 2" });
    assert.equal(r.error, false, r.text);
    // A statement the database refuses is an error, not a refused change.
    r = await first.call("run_query", { connection: "Shop", sql: "SELECT * FROM nope" });
    assert.equal(r.error, true, r.text);
  } finally {
    first.close();
  }
  const second = await startMcp(dir, "codex");
  try {
    assert.equal((await second.call("list_connections")).error, false);
    const r = await second.call("run_query", { connection: "Shop", sql: "SELECT id FROM orders WHERE id <= 5" });
    assert.equal(r.error, false, r.text);
  } finally {
    second.close();
  }
  const third = await startMcp(dir, HOSTILE);
  try {
    assert.equal((await third.call("list_connections")).error, false);
  } finally {
    third.close();
  }
}

async function waitForFile(page, path) {
  for (let i = 0; i < 100 && !existsSync(path); i++) await page.sleep(100);
  assert.ok(existsSync(path), `${path} was written`);
}

/** Clicks an export button with the save dialog answering `path`; the file's text. */
async function exportTo(page, action, path) {
  await answerSave(page, path);
  await page.eval(`document.querySelector('[data-action=${JSON.stringify(action)}]').click()`);
  await waitForFile(page, path);
  // The notice says where it went once the write is done.
  await page.waitFor(`document.querySelector('[data-notice="exported"]')?.textContent.includes(${JSON.stringify(path.split(/[\\/]/).pop())})`);
  return readFileSync(path, "utf8");
}

export default async function (page, { dir }) {
  await agentCalls(dir);

  await page.waitFor(`document.querySelector('.sidebar-header') !== null`);
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Agents");
  await page.eval(`document.querySelector('[data-agents-page="activity"]').click()`);
  await page.waitFor(`${panel}.hidden === false`);
  await page.waitFor(`${callTools}.length === 9`);

  // Folded: nothing asked of the store, and the call list is right there.
  assert.equal(await page.eval(`${report}`), null);
  assert.equal(await page.eval(`document.querySelector('[data-summary]').open`), false);

  await page.eval(`document.querySelector('[data-summary] > summary').click()`);
  await page.waitFor(`document.querySelectorAll('[data-agent-report] tbody tr').length === 5`);
  const table = await page.eval(rowsOf);
  const kinds = table.map((r) => r[0]);
  assert.deepEqual(kinds, ["agent", "session", "session", "agent", "session"]);
  // The agent's name is text: the tag is not in the page.
  assert.equal(table[0][1], HOSTILE);
  assert.equal(await page.eval(`document.querySelector('[data-agent-report] img')`), null);
  assert.equal(table[3][1], "codex");
  for (const i of [1, 2, 4]) assert.match(table[i][1], /^[0-9a-f]{8}…$/);
  //                          calls tables rows allowed refused errors
  assert.deepEqual(table[0].slice(2), ["7", "3", "9", "1", "1", "2"], "the agent, over both its sessions");
  assert.deepEqual(table[1].slice(2), ["1", "0", "0", "0", "0", "0"], "its newest session");
  assert.deepEqual(table[2].slice(2), ["6", "3", "9", "1", "1", "2"], "its first session");
  assert.deepEqual(table[3].slice(2), ["2", "1", "5", "0", "0", "0"]);
  assert.deepEqual(table[4].slice(2), ["2", "1", "5", "0", "0", "0"]);
  // The call list is still there, below it.
  assert.equal(await page.eval(`${callTools}.length`), 9);
  assert.equal(
    await page.eval(`(() => {
      const s = document.querySelector('[data-agent-report] .agent-report-scroll').getBoundingClientRect();
      return s.height <= 260;
    })()`),
    true,
    "the summary scrolls inside its own box",
  );

  // It follows the filters: only the calls that ended in an error. The refused change and the
  // failed statement are the two; the failed statement read no table.
  await page.eval(setValue(`document.querySelector('[data-filter="status"]')`, "error"));
  await page.waitFor(`document.querySelectorAll('[data-agent-report] tbody tr').length === 2`);
  const errors = await page.eval(rowsOf);
  assert.deepEqual(errors[0].slice(0, 2), ["agent", HOSTILE]);
  assert.deepEqual(errors[0].slice(2), ["2", "0", "0", "0", "1", "2"]);
  // A search that matches nothing leaves an empty summary and disabled exports.
  await page.eval(setValue(`document.querySelector('[data-filter="text"]')`, "no-such-text-anywhere"));
  await page.waitFor(`document.querySelector('[data-agent-report] tbody') === null`);
  assert.match(await page.eval(`${report}.textContent`), /No calls to summarize/);
  assert.equal(await page.eval(`document.querySelector('[data-action="export-report-md"]').disabled`), true);
  assert.equal(await page.eval(`document.querySelector('[data-action="export-report-html"]').disabled`), true);
  await page.eval(setValue(`document.querySelector('[data-filter="text"]')`, ""));
  await page.eval(setValue(`document.querySelector('[data-filter="status"]')`, ""));
  await page.waitFor(`document.querySelectorAll('[data-agent-report] tbody tr').length === 5`);

  // Markdown: one table whose rows hold together, the name escaped, the figures as shown.
  const md = await exportTo(page, "export-report-md", join(dir, "agent-report.md"));
  assert.ok(md.startsWith("# Agent report\n"), md);
  assert.ok(!md.includes(HOSTILE), "the tag is not written as it is");
  assert.ok(md.includes("&lt;img src=x onerror=alert(1)&gt;"));
  const lines = md.split("\n").filter((l) => l.startsWith("|"));
  assert.equal(lines.length, 2 + 5);
  const cells = (l) => l.replace(/\\\|/g, "").split("|").length;
  assert.deepEqual(new Set(lines.map(cells)), new Set([cells(lines[0])]));
  const agentLine = lines.find((l) => l.includes("**&lt;img"));
  assert.ok(agentLine.includes("| 7 | 3 | 9 | 1 | 1 | 2 |"), agentLine);
  assert.ok(lines.find((l) => l.includes("**codex**")).includes("| 2 | 1 | 5 | 0 | 0 | 0 |"));
  assert.match(md, /estimate/);

  // HTML: one file, the name inert, no script, nothing fetched from outside.
  const html = await exportTo(page, "export-report-html", join(dir, "agent-report.html"));
  assert.ok(html.startsWith("<!doctype html>"));
  assert.ok(!html.includes(HOSTILE));
  assert.ok(!/<img/i.test(html));
  assert.ok(html.includes("&#60;img src=x onerror=alert(1)&#62;"));
  assert.ok(!/<script/i.test(html));
  assert.ok(!/<(link|iframe|object|embed)/i.test(html));
  assert.ok(!/url\(|@import|https?:\/\//i.test(html));
  assert.match(html, /<td class="n">7<\/td><td class="n">3<\/td><td class="n">9<\/td><td class="n">1<\/td><td class="n">1<\/td><td class="n">2<\/td>/);

  // Parsed as a browser reads it (nothing runs in a parsed document): no image, no script, the
  // name is the text of a cell.
  const parsed = await page.eval(`(() => {
    const doc = new DOMParser().parseFromString(${JSON.stringify(html)}, 'text/html');
    return { images: doc.querySelectorAll('img, script, [onerror]').length, rows: doc.querySelectorAll('tbody tr').length, first: doc.querySelector('tbody tr td').textContent };
  })()`);
  assert.deepEqual(parsed, { images: 0, rows: 5, first: HOSTILE });

  // The CSV next to them is unchanged by all this.
  const csv = await page.eval(`window.__TAURI_INTERNALS__.invoke('agent_calls_export', { query: {
    text: null, session: null, status: null, before_id: null, limit: 1 } })`);
  assert.equal(csv.trim().split("\r\n").length, 1 + 9);

  // Clearing the log empties the summary.
  await page.eval(`document.querySelector('[data-action="clear-calls"]').click()`);
  await page.waitFor(`document.querySelector('#confirm-message') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Delete').click()`);
  await page.waitFor(`${callTools}.length === 0`);
  await page.waitFor(`/No calls to summarize/.test(${report}?.textContent ?? '')`);
}
