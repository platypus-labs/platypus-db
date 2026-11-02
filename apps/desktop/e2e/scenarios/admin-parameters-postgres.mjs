// Group J against the dev PostgreSQL: Server activity's Parameters view (pg_settings). The
// role platypus is given work_mem, so a new session has a setting that is not the default;
// the view lists the settings with their units and defaults, searches by name and
// description, keeps only the changed ones, shows the details of one and opens its
// ALTER SYSTEM SET in the editor without running it, and says that a fixed setting cannot be
// set. See the Parameters component in src/admin/Parameters.tsx.
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

const DB = "pg";
const NAME = "QA J PostgreSQL parameters";

export const skip = needs(DB);

export function setup({ dir }) {
  sh(DB, `ALTER ROLE platypus SET work_mem = '12MB';`);
  connections(dir, [[NAME, DB]]);
}

const count = (page) => page.eval(`${adminTab}.querySelectorAll('table.admin-parameters tbody tr').length`);
const countText = (page) => page.eval(`${adminTab}.querySelector('[data-field="parameter-count"]').textContent`);
const onlyChanged = `${adminTab}.querySelector('.admin-toolbar input[type=checkbox]')`;

export default async function (page) {
  try {
    await openAdmin(page, NAME, DB);
    // The view is read when it is opened: pg_settings has hundreds of rows, with units.
    await showSection(page, "Parameters");
    await page.waitFor(`${parameterRow("work_mem")} != null`, 30_000);
    const all = await count(page);
    assert.ok(all > 200, `pg_settings lists its settings: ${all}`);
    // One body, with no empty statistics block under it, and the list fills the height.
    assert.equal(await page.eval(`${adminTab}.querySelectorAll('.object-body').length`), 1);
    assert.equal(
      await page.eval(`(() => {
        const list = ${adminTab}.querySelector('.admin-list').getBoundingClientRect();
        const body = ${adminTab}.querySelector('.object-body').getBoundingClientRect();
        return body.bottom - list.bottom < 40;
      })()`),
      true,
      "the parameters list reaches the bottom of the tab",
    );
    assert.deepEqual(await parameterNames(page), [...(await parameterNames(page))].sort(), "sorted by name");

    // A value reads with its unit, next to its default and where it comes from.
    assert.equal(await parameterCell(page, "work_mem", 1), "12288 kB");
    assert.equal(await parameterCell(page, "work_mem", 2), "4096 kB");
    assert.equal(await parameterCell(page, "work_mem", 4), "user", "context: any user can set it");
    assert.equal(await parameterCell(page, "max_connections", 1), "100");
    assert.equal(await parameterCell(page, "max_connections", 4), "postmaster", "a restart changes it");

    // The search keeps the settings that have every word, in the name or the description.
    await setSearch(page, "logical_decoding_work_mem");
    await page.waitFor(`${parameterRow("logical_decoding_work_mem")} != null && ${parameterRow("max_connections")} == null`);
    assert.equal(await countText(page), "1 parameter");
    await setSearch(page, "WORK MEM");
    await page.waitFor(`${parameterRow("work_mem")} != null && ${parameterRow("maintenance_work_mem")} != null`);
    await setSearch(page, "no parameter is called like this");
    await page.waitFor(`${adminTab}.querySelector('.admin-empty')?.textContent === 'No parameters match.'`);
    assert.equal(await count(page), 0);
    await setSearch(page, "");
    await page.waitFor(`${adminTab}.querySelectorAll('table.admin-parameters tbody tr').length === ${all}`);

    // Only the changed: work_mem (role setting) and the options docker/compose.yml starts
    // the server with; every row left is marked, and max_connections is not among them.
    await page.eval(`${onlyChanged}.click()`);
    await page.waitFor(`${adminTab}.querySelectorAll('table.admin-parameters tbody tr').length < ${all}`);
    const changed = await parameterNames(page);
    assert.ok(changed.includes("work_mem"), `changed: ${changed}`);
    assert.ok(changed.includes("shared_preload_libraries"), `changed: ${changed}`);
    assert.ok(!changed.includes("max_connections"));
    assert.ok(
      await page.eval(
        `[...${adminTab}.querySelectorAll('table.admin-parameters tbody tr')].every(r => r.classList.contains('changed') || r.querySelector('.admin-own'))`,
      ),
      "each row left differs from its default",
    );
    // The filter and the search work together.
    await setSearch(page, "work_mem");
    await page.waitFor(`${parameterRow("shared_preload_libraries")} == null && ${parameterRow("work_mem")} != null`);
    await setSearch(page, "");
    await page.eval(`${onlyChanged}.click()`);
    await page.waitFor(`${adminTab}.querySelectorAll('table.admin-parameters tbody tr').length === ${all}`);

    // The details of a setting, and its statement in the editor: opened, never run.
    await page.eval(`${parameterRow("work_mem")}.click()`);
    await page.waitFor(`${adminTab}.querySelector('.admin-details-title')?.textContent === 'work_mem'`);
    assert.equal(await page.eval(field("value")), "12288 kB");
    assert.equal(await page.eval(field("default")), "4096 kB");
    assert.match(await page.eval(field("limits")), /64 – 2147483647/);
    assert.equal(
      await page.eval(`${adminTab}.querySelector('[data-field="parameter-statement"]').textContent`),
      "ALTER SYSTEM SET work_mem = 12288;",
    );
    const tabs = await page.eval(`document.querySelectorAll('.tab').length`);
    await page.eval(`${adminButton("Open in the editor")}.click()`);
    await page.waitFor(`document.querySelectorAll('.tab').length > ${tabs}`);
    await page.waitFor(`${editorText}.includes('ALTER SYSTEM SET work_mem = 12288;')`, 15_000);
    assert.equal(sh(DB, `SELECT count(*) FROM pg_file_settings WHERE name = 'work_mem';`), "0", "nothing was written");

    // Back to the admin tab: the list is as it was left. A setting fixed at build time has
    // no statement, and says why.
    await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.includes('Server activity')).click()`);
    await page.waitFor(`${adminTab} != null && ${parameterRow("block_size")} != null`, 15_000);
    await page.eval(`${parameterRow("block_size")}.click()`);
    await page.waitFor(`${adminTab}.querySelector('.admin-details-title')?.textContent === 'block_size'`);
    assert.equal(await page.eval(`${adminTab}.querySelector('[data-field="parameter-statement"]')`), null);
    assert.match(await page.eval(`${adminTab}.querySelector('.admin-details').textContent`), /does not let this parameter be set/);
    assert.equal(await page.eval(`${adminButton("Open in the editor")}`), undefined);

    // The details panel resizes with the keyboard (Left grows it) and Home restores it.
    const detailsWidth = `${adminTab}.querySelector('.admin-details').getBoundingClientRect().width`;
    const resizer = `${adminTab}.querySelector('.admin-split .panel-resizer')`;
    const press = (key) =>
      page.eval(`${resizer}.dispatchEvent(new KeyboardEvent('keydown', { key: ${JSON.stringify(key)}, bubbles: true, cancelable: true }))`);
    const base = await page.eval(detailsWidth);
    await press("ArrowLeft");
    await page.waitFor(`${detailsWidth} > ${base}`);
    await press("Home");
    await page.waitFor(`Math.abs(${detailsWidth} - ${base}) < 1`);

    // Reading again keeps the view.
    await page.eval(`${adminTab}.querySelector('button[aria-label="Refresh now"]').click()`);
    await page.waitFor(`${adminTab}.querySelector('button[aria-label="Refresh now"]').disabled === false`, 20_000);
    assert.equal(await count(page), all);
    assert.equal(await page.eval(`${adminTab}.querySelector('[data-field="parameters-error"]')`), null);
  } finally {
    sh(DB, `ALTER ROLE platypus RESET work_mem;`);
  }
}
