// Group S against the dev SQL Server: Server activity's Parameters view (sys.configurations).
// For the run two options are changed: "cost threshold for parallelism" (dynamic, set and
// reconfigured) and "fill factor (%)" (needs a restart: set, it waits as pending). The view
// lists the options sorted by name with their units and the defaults the app knows, searches
// by name and description, keeps only the changed ones (the one waiting for a restart too),
// shows the details with the sp_configure + RECONFIGURE statement, which opens in the editor
// and is not run, marks the pending one with its configured value and the restart context,
// and keeps the view when read again. See the Parameters component in src/admin/Parameters.tsx.
import assert from "node:assert/strict";
import { connections, needs, sh } from "../qa-grid.mjs";
import { editorText } from "../qa-ana.mjs";
import {
  adminButton,
  adminTab,
  field,
  openAdmin,
  parameterCell,
  parameterNames,
  parameterRow,
  setSearch,
  showSection,
} from "../admin.mjs";

const DB = "mssql";
const NAME = "QA S SQL Server parameters";

export const skip = needs(DB);

const q = (sql) => sh(DB, `SET NOCOUNT ON; ${sql}`);
const config = (name) => q(`SELECT CAST(value AS varchar(20)) + '|' + CAST(value_in_use AS varchar(20)) FROM sys.configurations WHERE name = '${name}'`);

const RESET = `EXEC sp_configure 'show advanced options', 1; RECONFIGURE;
EXEC sp_configure 'cost threshold for parallelism', 5;
EXEC sp_configure 'fill factor (%)', 0;
RECONFIGURE;
EXEC sp_configure 'show advanced options', 0; RECONFIGURE;`;

export function setup({ dir }) {
  sh(
    DB,
    `EXEC sp_configure 'show advanced options', 1; RECONFIGURE;
EXEC sp_configure 'cost threshold for parallelism', 20;
EXEC sp_configure 'fill factor (%)', 80;
RECONFIGURE;
EXEC sp_configure 'show advanced options', 0; RECONFIGURE;`,
  );
  connections(dir, [[NAME, DB]]);
}

const rows = (page) => page.eval(`${adminTab}.querySelectorAll('table.admin-parameters tbody tr').length`);
const countText = (page) => page.eval(`${adminTab}.querySelector('[data-field="parameter-count"]').textContent`);
const onlyChanged = `${adminTab}.querySelector('.admin-toolbar input[type=checkbox]')`;

async function select(page, name) {
  await page.waitFor(`${parameterRow(name)} != null`, 15_000);
  await page.eval(`${parameterRow(name)}.click()`);
  await page.waitFor(`${adminTab}.querySelector('.admin-details-title')?.textContent === ${JSON.stringify(name)}`);
}

export default async function (page) {
  try {
    assert.equal(config("cost threshold for parallelism"), "20|20");
    assert.equal(config("fill factor (%)"), "80|0", "waits for a restart");
    await openAdmin(page, NAME, DB);
    await showSection(page, "Parameters");
    await page.waitFor(`${parameterRow("cost threshold for parallelism")} != null`, 30_000);
    const all = await rows(page);
    assert.ok(all > 50 && all < 500, `sys.configurations lists its options: ${all}`);
    assert.equal(await countText(page), `${all} parameters`);
    const names = await parameterNames(page);
    assert.deepEqual(names, [...names].sort(), "sorted by name");

    // A value with its unit and the default the app knows; the context says what a change needs.
    assert.equal(await parameterCell(page, "cost threshold for parallelism", 1), "20");
    assert.equal(await parameterCell(page, "cost threshold for parallelism", 2), "5");
    assert.equal(await parameterCell(page, "cost threshold for parallelism", 4), "dynamic");
    assert.equal(await parameterCell(page, "max server memory (MB)", 1), "2147483647 MB");
    assert.equal(await parameterCell(page, "max server memory (MB)", 2), "2147483647 MB");
    assert.equal(await parameterCell(page, "fill factor (%)", 1), "0 %", "the value in use, not the configured");
    assert.equal(await parameterCell(page, "fill factor (%)", 3), "configured: 80");
    assert.equal(await parameterCell(page, "fill factor (%)", 4), "restart");
    assert.equal(
      await page.eval(`${parameterRow("fill factor (%)")}.querySelector('.admin-own') != null`),
      true,
      "pending is marked",
    );
    assert.equal(await page.eval(`${parameterRow("max server memory (MB)")}.querySelector('.admin-own') != null`), false);

    // The search: every word, in the name or the description.
    await setSearch(page, "cost threshold");
    await page.waitFor(`${parameterRow("cost threshold for parallelism")} != null && ${parameterRow("fill factor (%)")} == null`);
    await setSearch(page, "PARALLELISM COST");
    await page.waitFor(`${parameterRow("cost threshold for parallelism")} != null`);
    await setSearch(page, "no option is called like this");
    await page.waitFor(`${adminTab}.querySelector('.admin-empty')?.textContent === 'No parameters match.'`);
    assert.equal(await rows(page), 0);
    assert.equal(await countText(page), "0 parameters");
    await setSearch(page, "");
    await page.waitFor(`${adminTab}.querySelectorAll('table.admin-parameters tbody tr').length === ${all}`);

    // Only the changed: the dynamic one and the one waiting for a restart; the others are as built.
    await page.eval(`${onlyChanged}.click()`);
    await page.waitFor(`${adminTab}.querySelectorAll('table.admin-parameters tbody tr').length < ${all}`);
    const changed = await parameterNames(page);
    assert.ok(changed.includes("cost threshold for parallelism"), `changed: ${changed}`);
    assert.ok(changed.includes("fill factor (%)"), `a pending change shows with the changed: ${changed}`);
    assert.ok(!changed.includes("max server memory (MB)"));
    await setSearch(page, "fill");
    await page.waitFor(`${parameterRow("cost threshold for parallelism")} == null && ${parameterRow("fill factor (%)")} != null`);
    await setSearch(page, "");
    await page.eval(`${onlyChanged}.click()`);
    await page.waitFor(`${adminTab}.querySelectorAll('table.admin-parameters tbody tr').length === ${all}`);

    // The details of a dynamic option, and its statement in the editor: opened, never run.
    await select(page, "cost threshold for parallelism");
    assert.equal(await page.eval(field("value")), "20");
    assert.equal(await page.eval(field("default")), "5");
    assert.match(await page.eval(field("limits")), /^0 – 32767$/);
    assert.equal(
      await page.eval(`${adminTab}.querySelector('[data-field="parameter-statement"]').textContent`),
      "EXEC sp_configure N'cost threshold for parallelism', 20;\nRECONFIGURE;",
    );
    const tabs = await page.eval(`document.querySelectorAll('.tab').length`);
    await page.eval(`${adminButton("Open in the editor")}.click()`);
    await page.waitFor(`document.querySelectorAll('.tab').length > ${tabs}`);
    await page.waitFor(`${editorText}.includes("sp_configure N'cost threshold for parallelism', 20;")`, 15_000);
    assert.equal(config("cost threshold for parallelism"), "20|20", "nothing was run");

    // The option waiting for a restart: the value in use, the configured one as its source.
    await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.includes('Server activity')).click()`);
    await page.waitFor(`${adminTab} != null && ${parameterRow("fill factor (%)")} != null`, 15_000);
    await select(page, "fill factor (%)");
    assert.equal(await page.eval(field("value")), "0 %");
    assert.equal(await page.eval(field("source")), "configured: 80");
    assert.equal(await page.eval(field("context")), "Changes after a restart of the server.");
    assert.match(
      await page.eval(`${adminTab}.querySelector('[data-field="parameter-statement"]').textContent`),
      /sp_configure N'fill factor \(%\)', 80;/,
      "the statement carries the configured value",
    );

    // Reading again keeps the view.
    await page.eval(`${adminTab}.querySelector('button[aria-label="Refresh now"]').click()`);
    await page.waitFor(`${adminTab}.querySelector('button[aria-label="Refresh now"]').disabled === false`, 20_000);
    assert.equal(await rows(page), all);
    assert.equal(await page.eval(`${adminTab}.querySelector('[data-field="parameters-error"]') != null`), false);
  } finally {
    sh(DB, RESET);
  }
}
