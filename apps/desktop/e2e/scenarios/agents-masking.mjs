// Sensitive columns reach agents masked: run_query and sample_rows read `••••••`, NULL stays
// NULL, and an alias or a function around the column changes nothing. In Settings > Agents >
// Security the user can show them on one connection, and only there; the choice is approved by
// being made in the app, while the same line written in the file by hand gets the agent nothing.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { noMcp, startMcp } from "../mcp.mjs";

const PEOPLE = "00000000-0000-4000-8000-0000000000e1";
const OTHER = "00000000-0000-4000-8000-0000000000e2";
const EMAIL = "ann@example.com";

const row = (name) => `document.querySelector('.agents-panel tr[data-connection=${JSON.stringify(name)}]')`;
const shown = `document.querySelector('.agent-detail [data-field="agent_values"]')`;

export const skip = () => noMcp();

export function setup({ dir }) {
  const entry = (id, name, file) =>
    `\n[[connections]]\nid = "${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${join(dir, file)}'\nsave_password = false\n`;
  for (const file of ["people.db", "other.db"]) {
    const db = new DatabaseSync(join(dir, file));
    db.exec(`CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, email TEXT);
             INSERT INTO people VALUES (1, 'Ann', '${EMAIL}'), (2, 'Bob', NULL);`);
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
  await page.waitFor(`${shown} !== null`);
}

/** The JSON result of `sql` on connection `name`. */
async function read(mcp, name, sql) {
  const r = await mcp.call("run_query", { connection: name, sql, format: "json" });
  assert.equal(r.error, false, r.text);
  return JSON.parse(r.text);
}

export default async function (page, { dir }) {
  const mcp = await startMcp(dir, "e2e-masking");
  try {
    // By default, on every way of asking for the column.
    const sample = await mcp.call("sample_rows", { connection: "People", schema: "main", table: "people" });
    assert.ok(!sample.text.includes(EMAIL), sample.text);
    assert.match(sample.text, /••••••/);
    assert.match(sample.text, /Sensitive columns are masked: email\./);
    for (const sql of [
      "SELECT email FROM people",
      "SELECT email AS contact FROM people",
      "SELECT upper(email) FROM people",
      "SELECT *, email AS contact FROM people",
    ]) {
      const text = (await mcp.call("run_query", { connection: "People", sql })).text;
      assert.ok(!text.toLowerCase().includes(EMAIL), `${sql}: ${text}`);
    }
    const masked = await read(mcp, "People", "SELECT id, name, email FROM people ORDER BY id");
    assert.deepEqual(masked.rows, [[1, "Ann", "••••••"], [2, "Bob", null]]);
    assert.deepEqual(masked.masked_columns, ["email"]);

    // Shown on one connection from the app: only there, and agents read them at once.
    await chooseInSecurity(page, "People");
    assert.equal(await page.eval(`${shown}.checked`), false);
    await page.eval(`${shown}.click()`);
    await page.waitFor(`${shown}.checked === true`);
    const connections = () => readFileSync(join(dir, "connections.toml"), "utf8");
    assert.match(connections(), /agent_values = "shown"/);
    const open = await read(mcp, "People", "SELECT email FROM people ORDER BY id");
    assert.deepEqual(open.rows, [[EMAIL], [null]]);
    assert.equal(open.masked_columns, undefined);
    const other = await read(mcp, "Other", "SELECT email FROM people ORDER BY id");
    assert.deepEqual(other.rows, [["••••••"], [null]]);

    // The same line written by hand in the file for the other connection gets nothing.
    writeFileSync(
      join(dir, "connections.toml"),
      connections().replace(`id = "${OTHER}"`, `id = "${OTHER}"\nagent_values = "shown"`),
    );
    const edited = await read(mcp, "Other", "SELECT email FROM people ORDER BY id");
    assert.deepEqual(edited.rows, [["••••••"], [null]], "an edit outside the app waits for the user");
    writeFileSync(
      join(dir, "connections.toml"),
      connections().replace(`id = "${OTHER}"\nagent_values = "shown"`, `id = "${OTHER}"`),
    );

    // Masked again from the app.
    await page.eval(`${shown}.click()`);
    await page.waitFor(`${shown}.checked === false`);
    assert.doesNotMatch(connections(), /agent_values/);
    const again = await read(mcp, "People", "SELECT email FROM people ORDER BY id");
    assert.deepEqual(again.rows, [["••••••"], [null]]);
  } finally {
    await mcp.close?.();
  }
}
