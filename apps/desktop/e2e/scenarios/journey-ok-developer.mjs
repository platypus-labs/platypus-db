// QA journey (works, regression guard): a developer on PostgreSQL. A script saved to a file
// sets a variable and @includes a second file (relative to the first) that creates two
// tables in a new schema; the schema check finds the foreign key without an index, its fix
// opened in an editor and run, the check then clean; the table editor adds a column; fake
// data fills both tables with valid references; after a restart the script's tab is back
// with its file.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ALT, CTRL } from "../cdp.mjs";
import { connectWith, needsPostgres, openPath, pgConnection, psql, psqlTry, restart, status, treeRow, visibleTab } from "../journey-helpers.mjs";
import { menu } from "../navigator-helpers.mjs";
import { answerSave, generate } from "../transfer-helpers.mjs";

export const skip = needsPostgres;
const NAME = "QA jrn dev";
const SCRIPT = "@set s = qa_jrn_dev\nCREATE SCHEMA IF NOT EXISTS ${s};\n@include schema.sql\nSELECT count(*) AS n FROM ${s}.orders;";
const lint = `document.querySelector('.modal.schema-lint')`;
const editor = `[...document.querySelectorAll('.table-editor')].find(t => t.style.display !== 'none')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-content .cm-line')].map(l => l.textContent).join('\\n')`;
const resultTabs = `[...${visibleTab}.querySelectorAll('.result-tab')].map(t => t.textContent.replace(/×$/, ''))`;

export function setup({ dir }) {
  psql("SET lock_timeout = '10s'; DROP SCHEMA IF EXISTS qa_jrn_dev CASCADE;");
  writeFileSync(
    join(dir, "schema.sql"),
    "CREATE TABLE ${s}.customers (id serial PRIMARY KEY, name text NOT NULL);\n" +
      "CREATE TABLE ${s}.orders (id serial PRIMARY KEY, customer_id int REFERENCES ${s}.customers(id), total numeric(10,2));\n",
  );
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${pgConnection("00000000-0000-4000-8000-0000000a9002", NAME)}`);
}

async function runScript(page) {
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("x", "KeyX", 88, ALT);
  const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
  await page.waitFor(`${cancel}?.disabled === true && ${status} !== '' && !${status}.includes('Executing')`, 30_000);
}

export default async function (page, ctx) {
  try {
    await connectWith(page, NAME);
    await page.eval(`${treeRow(NAME)}.id = 'jrn-c'`);
    await page.clickSel("#jrn-c", { clickCount: 2 });
    await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
    await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
    await page.type(SCRIPT);
    await page.waitFor(`${editorText} === ${JSON.stringify(SCRIPT)}`);
    await page.key("Escape", "Escape", 27);
    const file = join(ctx.dir, "dev.sql");
    await answerSave(page, file);
    await page.key("s", "KeyS", 83, CTRL);
    await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('dev.sql')`, 10_000);
    assert.equal(readFileSync(file, "utf8"), SCRIPT);
    await runScript(page);
    assert.deepEqual(await page.eval(resultTabs), [
      "1. s = qa_jrn_dev",
      "2. 0 rows affected",
      "3. Included 2 statements",
      "4. 0 rows affected",
      "5. 0 rows affected",
      "6. 1 row",
    ]);

    // The new schema, checked and fixed.
    await menu(page, NAME, "Refresh");
    await openPath(page, [NAME, "qa_jrn_dev", "Tables"], "orders");
    await menu(page, "qa_jrn_dev", "Check schema…");
    await page.waitFor(`${lint}?.querySelectorAll('.lint-findings li').length === 1`, 15_000);
    assert.match(await page.eval(`${lint}.querySelector('.lint-findings li').textContent`), /customer_id/);
    await page.eval(`[...${lint}.querySelectorAll('button')].find(b => b.textContent === 'Open 1 fix in editor').click()`);
    await page.waitFor(`${editorText}.trim() === 'CREATE INDEX orders_customer_id_idx ON qa_jrn_dev.orders (customer_id);'`, 10_000);
    await runScript(page);
    await menu(page, "qa_jrn_dev", "Check schema…");
    await page.waitFor(`${lint}?.textContent.includes('No problems found.')`, 15_000);
    await page.key("Escape", "Escape", 27);
    await page.waitFor(`${lint} === null`);

    // The table editor adds a column.
    await menu(page, "customers", "Edit table…");
    await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 2`, 15_000);
    await page.eval(`${editor}.querySelector('.add-row').click()`);
    await page.waitFor(`${editor}.querySelectorAll('tbody tr').length === 3`);
    const cell = (c) => `${editor}.querySelectorAll('tbody tr')[2].querySelectorAll('td')[${c}].querySelector('input')`;
    for (const [c, text] of [[0, "email"], [1, "text"]]) {
      await page.eval(`(${cell(c)}).id = 'jrn-in'`);
      await page.clickSel("#jrn-in");
      await page.key("a", "KeyA", 65, CTRL);
      await page.type(text);
      await page.eval(`document.getElementById('jrn-in')?.removeAttribute('id')`);
    }
    await page.key("Escape", "Escape", 27);
    await page.waitFor(`${editor}.querySelector('.object-header button.primary')?.disabled === false`, 10_000);
    await page.eval(`${editor}.querySelector('.object-header button.primary').click()`);
    await page.waitFor(`document.querySelector('.modal.sql-action button[type=submit]')?.disabled === false`, 10_000);
    assert.match(await page.eval(`document.querySelector('.modal.sql-action pre').textContent`), /ADD COLUMN email text/i);
    await page.eval(`document.querySelector('.modal.sql-action button[type=submit]').click()`);
    await page.waitFor(`document.querySelector('.modal.sql-action') === null`, 15_000);
    assert.equal(psql("SELECT string_agg(column_name, ',' ORDER BY ordinal_position) FROM information_schema.columns WHERE table_schema = 'qa_jrn_dev' AND table_name = 'customers'"), "id,name,email");

    // Fake data with valid references.
    assert.match((await generate(page, "customers", ["qa_jrn_dev"], 50)).out, /Added 50 rows/);
    assert.match((await generate(page, "orders", ["qa_jrn_dev"], 200)).out, /Added 200 rows/);
    assert.equal(
      psql("SELECT (SELECT count(*) FROM qa_jrn_dev.customers) || ',' || (SELECT count(*) FROM qa_jrn_dev.orders) || ',' || (SELECT count(*) FROM qa_jrn_dev.orders o WHERE customer_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM qa_jrn_dev.customers c WHERE c.id = o.customer_id))"),
      "50,200,0",
    );

    // After a restart the script's tab is back with its file.
    await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith('dev.sql')).click()`);
    await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('dev.sql')`);
    await restart(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
    await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('dev.sql')`, 15_000);
    await page.waitFor(`${editorText} === ${JSON.stringify(SCRIPT)}`, 10_000);
  } finally {
    psqlTry("SET lock_timeout = '10s'; DROP SCHEMA IF EXISTS qa_jrn_dev CASCADE");
  }
}
