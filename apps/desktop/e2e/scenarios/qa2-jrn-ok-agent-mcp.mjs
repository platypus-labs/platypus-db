// QA journey (works, regression guard): an agent through platypus-mcp, driven by a JSON-RPC
// client, on a PostgreSQL connection the user made in the app from a pasted URL (password in
// the keychain, which the agent never sees): it lists the connection and reads data; its
// write is refused while the access is "Read only"; the user allows writes in the connection
// dialog and the same running server lets the next write through; the user marks the
// connection Production and the write is refused again (access capped at read only); the
// agent's statements show in the app's history with its name. The connection is deleted at
// the end (its keychain entry with it) and the agent no longer sees it.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { dialog, editConnection, fill, formField, paste, saveForm } from "../conn-helpers.mjs";
import { noMcp, startMcp } from "../mcp.mjs";
import { needsPostgres, psql, psqlTry, treeRow, visibleTab } from "../qa-jrn.mjs";
import { menu } from "../qa-nav-helpers.mjs";

export const ready = `document.querySelector('.sidebar-header') !== null`;
export const skip = async () => noMcp() ?? (await needsPostgres());
const NAME = "Agent PG";
const update = (v, id) => `UPDATE qa_jrn_agent SET v = '${v}' WHERE id = ${id}`;

export function setup() {
  psql("DROP TABLE IF EXISTS qa_jrn_agent; CREATE TABLE qa_jrn_agent (id int PRIMARY KEY, v text); INSERT INTO qa_jrn_agent VALUES (1, 'a'), (2, 'b');");
}

export default async function (page, ctx) {
  let mcp;
  try {
    await page.eval(`document.querySelector('.welcome-cta').click()`);
    await page.waitFor(`document.querySelector('.modal.driver-picker') !== null`);
    await page.eval(paste(`document.querySelector('.driver-picker [data-field="connection_url"]')`, "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable"));
    await page.waitFor(`${dialog} !== null`);
    await page.eval(fill(formField("Name"), NAME));
    await saveForm(page);
    assert.doesNotMatch(readFileSync(join(ctx.dir, "connections.toml"), "utf8"), /platypus:platypus|^password\s*=/m, "the password stays out of the file");

    mcp = await startMcp(ctx.dir, "qa-journey-agent");
    const run = (sql) => mcp.call("run_query", { connection: NAME, sql });
    const list = JSON.parse((await mcp.call("list_connections")).text);
    assert.deepEqual(list.map((c) => [c.name, c.agent_access]), [[NAME, "read_only"]]);
    const read = await run("SELECT * FROM qa_jrn_agent ORDER BY id");
    assert.equal(read.error, false, read.text);
    assert.match(read.text, /\(2 rows/);
    assert.match((await run(update("x", 1))).text, /read-only for agents/);

    // The user allows writes.
    await editConnection(page, NAME);
    await page.eval(fill(formField("Agent access"), "read_write"));
    await saveForm(page);
    const write = await run(update("x", 1));
    assert.equal(write.error, false, write.text);

    // Then marks it Production: read only for agents again.
    await editConnection(page, NAME);
    await page.eval(fill(formField("Type"), "prod"));
    await saveForm(page);
    const refused = await run(update("y", 2));
    assert.ok(refused.error && /read-only for agents/.test(refused.text), refused.text);
    assert.equal(psql("SELECT string_agg(id || v, ',' ORDER BY id) FROM qa_jrn_agent"), "1x,2b");

    // The agent's statements in the app's history, with its name.
    await page.eval(`${treeRow(NAME)}.id = 'jrn-c'`);
    await page.clickSel("#jrn-c", { clickCount: 2 });
    await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
    await page.eval(`[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'History').click()`);
    await page.waitFor(`[...document.querySelectorAll('.history-entry')].filter(e => e.textContent.includes('qa-journey-agent')).length === 4`, 10_000);

    // Deleted: gone for the agent too.
    await menu(page, NAME, "Delete…");
    await page.waitFor(`document.querySelector('.modal') !== null`);
    await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Delete').click()`);
    await page.waitFor(`${treeRow(NAME)} === undefined`, 10_000);
    assert.deepEqual(JSON.parse((await mcp.call("list_connections")).text), []);
  } finally {
    mcp?.close();
    psqlTry("SET lock_timeout = '10s'; DROP TABLE IF EXISTS qa_jrn_agent");
  }
}
