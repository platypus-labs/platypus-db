// BUG (medium, feature broken by default): "Generate data…" on a SQL Server table with an IDENTITY
// key suggests "next key" for it and fails: "Cannot insert explicit value for identity column in
// table … when IDENTITY_INSERT is set to OFF". suggest() in src/transfer/generators.ts leaves a
// column to the database only when it has a default; an IDENTITY column has none (the same
// holds for PostgreSQL GENERATED ALWAYS AS IDENTITY). A serial key works (it has a default).
//   Steps: CREATE TABLE … (id int IDENTITY PRIMARY KEY, …); Generate data…, 10 rows, Generate.
//   Expected: the key left to the database ("skip"), "Added 10 rows."
//   Actual: "Nothing was added: Cannot insert explicit value for identity column …".
// Needs SQL Server (docker compose -f docker/compose.yml --profile sqlserver up -d).
import assert from "node:assert/strict";
import { MSSQL, expand, mssqlEntry, needsPort, openEditor, rowMenu, run, saveConnections, treeRow } from "../perf.mjs";

export const skip = needsPort(MSSQL.port, "SQL Server");
const TABLE = "qa_perf_identity";
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

export function setup({ dir }) {
  saveConnections(dir, mssqlEntry());
}

export default async function (page) {
  await expand(page, MSSQL.name, MSSQL.password);
  await openEditor(page, MSSQL.name);
  await run(page, `DROP TABLE IF EXISTS dbo.${TABLE}`);
  await run(page, `CREATE TABLE dbo.${TABLE} (id int IDENTITY PRIMARY KEY, name nvarchar(100), born date)`);
  try {
    await page.waitFor(`${treeRow("platypus")} !== undefined`, 20_000);
    if ((await page.eval(`${treeRow("platypus")}.getAttribute('aria-expanded')`)) !== "true") {
      await page.eval(`${treeRow("platypus")}.querySelector('.twisty').click()`);
    }
    await page.waitFor(`${treeRow("dbo")} !== undefined`, 20_000);
    await rowMenu(page, "dbo", "Refresh");
    if ((await page.eval(`${treeRow("dbo")}.getAttribute('aria-expanded')`)) !== "true") {
      await page.eval(`${treeRow("dbo")}.querySelector('.twisty').click()`);
    }
    await page.waitFor(`[...document.querySelectorAll('.tree-row')].some(r => r.querySelector('.tree-label')?.textContent === 'Tables')`, 20_000);
    const tables = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === 'Tables' && r.getAttribute('aria-expanded') !== 'true')`;
    if (await page.eval(`${tables} !== undefined`)) await page.eval(`${tables}.querySelector('.twisty').click()`);
    await page.waitFor(`${treeRow(TABLE)} !== undefined`, 20_000);
    await rowMenu(page, TABLE, "Generate data");
    const dialog = `document.querySelector('.modal.generate-data')`;
    await page.waitFor(`${dialog} !== null && ${button(dialog, "Generate")} && !${button(dialog, "Generate")}.disabled`, 30_000);
    await page.eval(`(() => { const i = ${dialog}.querySelector('input[type=number]');
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(i, '10');
      i.dispatchEvent(new Event('input', { bubbles: true })); })()`);
    await page.waitFor(`${dialog}.querySelector('input[type=number]').value === '10'`);
    const generators = await page.eval(`[...${dialog}.querySelectorAll('select')].map(s => s.value)`);
    await page.eval(`${button(dialog, "Generate")}.click()`);
    await page.waitFor(`/Added [\\d.,]+ rows\\.|Nothing was added/.test(${dialog}.textContent)`, 60_000);
    const text = await page.eval(`${dialog}.textContent`);
    assert.match(text, /Added 10 rows\./, `generators ${generators.join(", ")}: ${text.match(/Nothing was added.*?(?=Cancel|Close|$)/)?.[0]}`);
    assert.equal(generators[0], "skip", "the IDENTITY key is left to the database");
    await page.eval(`${button(dialog, "Close")}.click()`);
  } finally {
    await page.eval(`[...document.querySelectorAll('.modal.generate-data button')].find(b => /^(Close|Cancel)$/.test(b.textContent.trim()))?.click()`);
    await run(page, `DROP TABLE IF EXISTS dbo.${TABLE}`);
  }
}
