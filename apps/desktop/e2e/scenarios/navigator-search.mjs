// Group C P2/P3 against the dev PostgreSQL: table sizes, metadata and data search,
// bookmarks, navigator filters, partitions under their table, and the metadata cache shown
// offline. Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { setup as pgSetup, skip as pgSkip } from "./dev-postgres.mjs";

export const skip = pgSkip;
export const setup = pgSetup;

const NAME = "E2E PG";
const visibleTab = `[...document.querySelectorAll('.query-tab, .object-tab')].find(t => t.style.display !== 'none')`;
const searchTab = `[...document.querySelectorAll('.search-tab')].find(t => t.style.display !== 'none')`;
const rows = (label) =>
  `[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const row = (label) => `${rows(label)}[0]`;

async function rightClick(page, label) {
  await page.eval(`${row(label)}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.eval(`document.getElementById('e2e-target').removeAttribute('id')`);
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

async function choose(page, item) {
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === ${JSON.stringify(item)}).click()`,
  );
}

async function search(page, text) {
  await page.waitFor(`${searchTab}?.querySelector('.search-input') != null`);
  await page.eval(`${searchTab}.querySelector('.search-input').focus()`);
  await page.type(text);
  await page.waitFor(`${searchTab}.querySelector('.search-input').value === ${JSON.stringify(text)}`);
  await page.eval(`${searchTab}.querySelector('.search-form button[type=submit]').click()`);
}

async function runInEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
  // Two DDL statements in a row print the same status, which React then leaves as it is: the
  // result's notice is new on every run, so the old one is marked and a new one waited for.
  await page.eval(`${visibleTab}.querySelectorAll('.notice').forEach(n => n.dataset.e2eOld = '1')`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(
    `(() => { const s = ${visibleTab}.querySelector('.statusbar span')?.textContent ?? ''; const fresh = ${visibleTab}.querySelector('.notice:not([data-e2e-old])'); return (s && !s.includes('Executing')) || (fresh !== null && !fresh.textContent.includes('Executing')); })()`,
    15_000,
  );
}

async function password(page) {
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
}

export default async function (page, { shots }) {
  const shot = (name) => shots && page.screenshot(`${shots}/navigator-search-${name}.png`);
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.click()`);
  await password(page);
  await page.waitFor(`${row("demo")} !== undefined`, 15_000);
  await page.eval(`${row("demo")}.click()`);
  await page.waitFor(`${row("orders")} !== undefined`, 15_000);

  // Tables show their estimated rows and size.
  assert.match(await page.eval(`${row("orders")}.querySelector('.tree-detail')?.textContent ?? ''`), /\d+(\.\d)? (B|KB|MB)$/);

  // Metadata search: names and columns; a hit opens the object.
  await rightClick(page, NAME);
  await choose(page, "Search objects…");
  await search(page, "customer");
  const results = `[...(${searchTab}?.querySelectorAll('.search-results tbody tr') ?? [])].map(r => r.textContent.trim())`;
  await page.waitFor(`${results}.some(t => t.startsWith('customers'))`, 15_000);
  assert.ok((await page.eval(results)).some((t) => t.startsWith("orders") && t.endsWith("column customer_id")));
  await shot("metadata");
  await page.eval(`[...${searchTab}.querySelectorAll('.search-results tbody tr')].find(r => r.textContent.trim().startsWith('customers')).click()`);
  await page.waitFor(`document.querySelector('.object-tab:not(.search-tab)[style*="flex"] .object-title strong')?.textContent === 'customers'`);

  // Data search in a schema: a table with hits opens filtered on them.
  await rightClick(page, "demo");
  await choose(page, "Search data…");
  await search(page, "user12@");
  await page.waitFor(`/tables searched/.test(${searchTab}?.querySelector('.object-body')?.textContent ?? '')`, 30_000);
  assert.ok((await page.eval(results)).some((t) => t.startsWith("demo.customers")));
  await shot("data");
  await page.eval(`[...${searchTab}.querySelectorAll('.search-results tbody tr')].find(r => r.textContent.trim().startsWith('demo.customers')).click()`);
  await page.waitFor(`/ILIKE '%user12@%'/.test(${visibleTab}?.querySelector('.cm-content')?.textContent ?? '')`);

  // A bookmark shows in its own folder under the connection.
  await rightClick(page, "orders");
  await choose(page, "Add bookmark");
  await page.waitFor(`${row("Bookmarks")} !== undefined && ${rows("orders")}.length === 2`);

  // Navigator filters hide matching objects; the bookmark stays.
  await rightClick(page, NAME);
  await choose(page, "Navigator filters…");
  await page.waitFor(`document.querySelector('.modal.navigator-filter') !== null`);
  await page.eval(`document.querySelectorAll('.modal.navigator-filter input')[1].focus()`);
  await page.type("ORD*");
  await page.eval(`document.querySelector('.modal.navigator-filter button[type=submit]').click()`);
  await page.waitFor(`${rows("orders")}.length === 1 && /filtered/.test(${row(NAME)}.textContent)`);
  await rightClick(page, NAME);
  await choose(page, "Navigator filters…");
  await page.waitFor(`document.querySelector('.modal.navigator-filter') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal.navigator-filter button')].find(b => b.textContent === 'Remove filters').click()`);
  await page.waitFor(`${rows("orders")}.length === 2`);

  // A partition shows under its table, not among the tables.
  const parent = `e2e_part_${Date.now()}`;
  await rightClick(page, NAME);
  await choose(page, "New SQL script");
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`);
  await runInEditor(page, `CREATE TABLE demo.${parent} (at date NOT NULL) PARTITION BY RANGE (at)`);
  await runInEditor(page, `CREATE TABLE demo.${parent}_2026 PARTITION OF demo.${parent} FOR VALUES FROM ('2026-01-01') TO ('2027-01-01')`);
  await rightClick(page, "demo");
  await choose(page, "Refresh");
  await page.waitFor(`${row(parent)} !== undefined`, 15_000);
  assert.equal(await page.eval(`${row(`${parent}_2026`)} === undefined`), true);
  await page.eval(`${row(parent)}.click()`);
  await page.waitFor(`${row("Partitions")} !== undefined`, 15_000);
  await page.eval(`${row("Partitions")}.click()`);
  await page.waitFor(`${row(`${parent}_2026`)} !== undefined`);
  await shot("partitions");
  await runInEditor(page, `DROP TABLE demo.${parent}`);

  // Disconnected, the tree comes back from the disk cache; without a session it stays offline.
  await new Promise((r) => setTimeout(r, 1500)); // the cache is written a second after the last load
  await rightClick(page, NAME);
  await choose(page, "Disconnect");
  await page.waitFor(`${row("orders")} === undefined`);
  await page.eval(`${row(NAME)}.click()`);
  await page.waitFor(`${row("orders")} !== undefined && document.querySelector('.modal input[type=password]') !== null`, 15_000);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`/offline/.test(${row(NAME)}.textContent) && ${row("orders")} !== undefined`);
  // Offline, nothing asks for the password again, e.g. to load the current schema.
  await new Promise((r) => setTimeout(r, 500));
  assert.equal(await page.eval(`document.querySelector('.modal') === null`), true);
  await shot("offline");
}
