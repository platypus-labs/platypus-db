// Regression guard (passes): PostgreSQL objects with odd names work from the navigator — a
// schema with a space and quotes, tables with mixed case, a quote, a slash, a dot, unicode and
// a reserved word: Open data reads their rows, the DDL quotes them, the filter box finds them,
// Rename… and Drop… run through their dialogs, and a multi-selection's Copy name lists all, bare.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { DBS, connectTo, expand, gridText, menu, needs, row, saveConnections, treeLabels, visibleTab, waitRun } from "../navigator-helpers.mjs";

export const skip = needs("postgres");

const SCHEMA = 'qa_nav ok "S"';
const Q = '"qa_nav ok ""S"""';
const objTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${Q} CASCADE; CREATE SCHEMA ${Q};
    CREATE TABLE ${Q}."Mixed Case" ("Id" int PRIMARY KEY); INSERT INTO ${Q}."Mixed Case" VALUES (1);
    CREATE TABLE ${Q}."quo""te" ("a""b" text); INSERT INTO ${Q}."quo""te" VALUES ('q');
    CREATE TABLE ${Q}."sl/ash.dot" (id int); INSERT INTO ${Q}."sl/ash.dot" VALUES (2);
    CREATE TABLE ${Q}."ünï_😀" (id int); INSERT INTO ${Q}."ünï_😀" VALUES (3);
    CREATE TABLE ${Q}."select" (id int); INSERT INTO ${Q}."select" VALUES (4);
    CREATE TABLE ${Q}.to_rename (id int);`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a0017"));
}

async function openData(page, table, expected) {
  await menu(page, table, "Open data");
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null && document.querySelector('.tab.active')?.textContent.startsWith(${JSON.stringify(table)})`, 10_000);
  await waitRun(page);
  assert.equal(await gridText(page), expected, table);
}

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, SCHEMA, "Mixed Case");
    await openData(page, "Mixed Case", "Id\n1");
    // The copy quotes a header that holds a quote.
    await openData(page, 'quo"te', '"a""b"\nq');
    await openData(page, "sl/ash.dot", "id\n2");
    await openData(page, "ünï_😀", "id\n3");
    await openData(page, "select", "id\n4");

    await menu(page, 'quo"te', "Open details");
    await page.waitFor(`${objTab}?.querySelector('.object-title strong')?.textContent === 'quo"te'`, 10_000);
    await page.eval(`[...${objTab}.querySelectorAll('.object-sections button')].find(b => b.textContent === 'DDL').click()`);
    await page.waitFor(`(${objTab}.querySelector('.object-ddl .cm-content')?.textContent ?? '').includes('CREATE TABLE ${Q.replaceAll('"', '\\"')}."quo""te"')`, 10_000);

    // The filter box finds unicode and is case-insensitive.
    await page.eval(`document.querySelector('.tree-filter').focus()`);
    await page.type("ÜNÏ");
    await page.waitFor(`${row("ünï_😀")} !== undefined && ${row("select")} === undefined`);
    await page.key("Escape", "Escape", 27);
    await page.waitFor(`${row("select")} !== undefined`);

    // Rename to an odd name and drop it.
    await menu(page, "to_rename", "Rename…");
    await page.waitFor(`document.querySelector('.modal.sql-action input') !== null`);
    await page.eval(`(() => { const i = document.querySelector('.modal.sql-action input'); i.focus(); i.select(); })()`);
    await page.type('re "named"');
    await page.waitFor(`/RENAME TO "re ""named""";/.test(document.querySelector('.sql-preview').textContent)`);
    await page.eval(`document.querySelector('.modal.sql-action button[type=submit]').click()`);
    await page.waitFor(`${row('re "named"')} !== undefined && ${row("to_rename")} === undefined`, 15_000);
    await menu(page, 're "named"', "Drop…");
    await page.waitFor(`/DROP TABLE ${Q.replaceAll('"', '\\"')}\\."re ""named""";/.test(document.querySelector('.sql-preview')?.textContent ?? '')`, 10_000);
    await page.eval(`document.querySelector('.modal.sql-action button[type=submit]').click()`);
    await page.waitFor(`${row('re "named"')} === undefined && document.querySelector('.modal.sql-action') === null`, 15_000);

    // Ctrl+click selects several; Copy name copies them all.
    await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }; window.__copied = undefined`);
    for (const [label, modifiers] of [["Mixed Case", 0], ["select", CTRL]]) {
      await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-c'`);
      await page.clickSel("#e2e-c", { modifiers });
      await page.eval(`document.getElementById('e2e-c').removeAttribute('id')`);
    }
    await menu(page, "select", "Copy name");
    await page.waitFor(`window.__copied !== undefined`);
    // Copy name is the bare name; Copy qualified name has the schema and the quotes.
    assert.equal(await page.eval(`window.__copied`), "Mixed Case\nselect");
    assert.ok((await page.eval(treeLabels)).includes("sl/ash.dot"));
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${Q} CASCADE;`);
  }
}
