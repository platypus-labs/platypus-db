// Elasticsearch from the `search` profile of docker/compose.yml, in the navigator: indices are
// tables (with their health and shards in the tooltip, system ones marked, closed ones marked),
// an alias and a data stream are views, hidden indices (and the backing indices of a data
// stream) are not listed, the columns are the fields of the mapping (`a.b` for objects,
// `title.raw` for multi-fields, aliases of fields), the context menu offers only what works on
// an index, an index pattern in the connection narrows the list, Generate SELECT writes the
// search, and a double click opens the documents in the grid with the document tree.
// Skipped when Elasticsearch is not running.
import assert from "node:assert/strict";
import { gridTsv, runStatement, editorText, treeRow, visibleTab } from "../qa-sql.mjs";
import { closeMenu, connection, drop, es, expandCluster, openIndex, rowMenu, saveConnections, seed, skip as skipSearch, table } from "../search-helpers.mjs";
import { revealItem, submenuItems } from "../menu-sub.mjs";

const NAME = "E2E ES navigator";
const NARROW = "E2E ES pattern";

export const skip = skipSearch;
export const setup = async ({ dir }) => {
  saveConnections(dir, connection(10, NAME), connection(11, NARROW, "", { database: "e2e_nav_or*" }));
  await cleanup();
  await seed(
    "e2e_nav_orders",
    {
      properties: {
        title: { type: "text", fields: { raw: { type: "keyword" } } },
        customer: { properties: { name: { type: "keyword" }, address: { properties: { zip: { type: "keyword" } } } } },
        qty: { type: "long" },
        price: { type: "double" },
        ok: { type: "boolean" },
        short: { type: "alias", path: "qty" },
        tags: { type: "keyword" },
      },
    },
    [
      ["o1", { title: "First order", customer: { name: "Ada", address: { zip: "00100" } }, qty: 3, price: 9.5, ok: true, tags: ["a", "b"] }],
      ["o2", { title: "Second order", customer: { name: "Bob", address: { zip: "20100" } }, qty: 5, price: 12, ok: false, tags: ["c"] }],
      ["o3", { title: "Third order", customer: { name: "Cy" }, qty: 8, price: 0.25, ok: true }],
    ],
  );
  await seed("e2e_nav_other", { properties: { v: { type: "keyword" } } }, [{ v: "x" }]);
  await seed("e2e_nav_hidden", { properties: { v: { type: "keyword" } } }, [{ v: "x" }], { "index.hidden": true });
  await seed("e2e_nav_closed", { properties: { v: { type: "keyword" } } }, [{ v: "x" }]);
  await es("POST", "/e2e_nav_closed/_close");
  await seed(".e2e_nav_system", { properties: { v: { type: "keyword" } } }, [{ v: "x" }]);
  await es("POST", "/_aliases", { actions: [{ add: { index: "e2e_nav_orders", alias: "e2e_nav_alias" } }] });
  await es("PUT", "/_index_template/e2e_nav_ds", { index_patterns: ["e2e_nav_ds*"], data_stream: {}, priority: 500, template: { settings: { number_of_replicas: 0 } } });
  await es("PUT", "/_data_stream/e2e_nav_ds");
};

async function cleanup() {
  await es("DELETE", "/_data_stream/e2e_nav_ds", undefined, { allow: [404] });
  await es("DELETE", "/_index_template/e2e_nav_ds", undefined, { allow: [404] });
  await drop("e2e_nav_orders", "e2e_nav_other", "e2e_nav_hidden", "e2e_nav_closed", ".e2e_nav_system");
}

/** The tooltip of a tree row. */
const tip = (page, label) => page.eval(`${treeRow(label)}.title`);

export default async function (page) {
  await expandCluster(page, NAME, "e2e_nav_orders");
  for (const index of ["e2e_nav_other", "e2e_nav_closed", ".e2e_nav_system"]) {
    await page.waitFor(`${treeRow(index)} !== undefined`, 15_000);
  }

  // Hidden indices are not listed, nor are the backing indices of a data stream.
  assert.ok(await page.eval(`${treeRow("e2e_nav_hidden")} === undefined`), "a hidden index is listed");
  assert.ok(await page.eval(`![...document.querySelectorAll('.tree-label')].some(l => l.textContent.startsWith('.ds-'))`), "a backing index is listed");

  // The tooltip says what the cluster says: system, closed, shards.
  assert.match(await tip(page, ".e2e_nav_system"), /system/);
  assert.match(await tip(page, "e2e_nav_closed"), /closed/);
  assert.match(await tip(page, "e2e_nav_orders"), /1 shard/);
  assert.match(await tip(page, "e2e_nav_orders"), /0 replicas/);

  // Aliases and data streams are views, in the Views folder (closed at first).
  assert.ok(await page.eval(`${treeRow("e2e_nav_alias")} === undefined`), "the Views folder is open");
  await page.eval(`${treeRow("Views")}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("e2e_nav_alias")} !== undefined && ${treeRow("e2e_nav_ds")} !== undefined`, 15_000);
  assert.match(await tip(page, "e2e_nav_alias"), /alias of e2e_nav_orders/);
  assert.match(await tip(page, "e2e_nav_ds"), /data stream/);

  // The columns of an index are the fields of its mapping, after _id.
  await page.eval(`${treeRow("e2e_nav_orders")}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("customer.address.zip")} !== undefined`, 15_000);
  const detail = (label) => page.eval(`${treeRow(label)}.querySelector('.tree-detail')?.textContent ?? ''`);
  assert.notEqual(await detail("_id"), "");
  assert.match(await detail("title"), /^text/);
  assert.match(await detail("title.raw"), /^keyword/);
  assert.match(await detail("customer.name"), /^keyword/);
  assert.match(await detail("customer.address.zip"), /^keyword/);
  assert.match(await detail("qty"), /^long/);
  assert.match(await detail("price"), /^double/);
  assert.match(await detail("ok"), /^boolean/);
  assert.match(await detail("short"), /^alias/);
  assert.match(await tip(page, "title.raw"), /sub-field of title/);
  assert.match(await tip(page, "short"), /alias of qty/);
  await page.eval(`${treeRow("e2e_nav_orders")}.querySelector('.twisty').click()`);

  // The menu of an index offers what works on one: reading, dropping; no SQL writes.
  const items = await rowMenu(page, "e2e_nav_orders");
  for (const item of ["Open data", "Generate SQL"]) assert.ok(items.includes(item), `${item} in ${items}`);
  assert.ok(items.some((i) => /^Drop/.test(i)), `Drop in ${items}`);
  for (const item of ["Visual query builder", "Generate INSERT", "Generate UPDATE", "Generate DELETE", "Generate UPSERT", "Truncate…", "Edit table…"]) {
    assert.ok(!items.includes(item), `no ${item} in ${items}`);
  }
  const sql = await submenuItems(page, "Generate SQL");
  for (const item of ["Generate INSERT", "Generate UPDATE", "Generate DELETE", "Generate UPSERT"]) assert.ok(!sql.some((i) => i.startsWith(item)), `no ${item} in ${sql}`);
  await closeMenu(page);
  // An alias reads, and cannot be dropped as a table.
  const viewItems = await rowMenu(page, "e2e_nav_alias");
  assert.ok(viewItems.includes("Generate SQL"), `Generate SQL in ${viewItems}`);
  assert.ok(!viewItems.some((i) => /^Drop|^Truncate/.test(i)), `no Drop or Truncate in ${viewItems}`);
  await closeMenu(page);

  // Generate SELECT writes the search of the index, and it runs.
  await rowMenu(page, "e2e_nav_orders");
  await revealItem(page, "Generate SELECT");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Generate SELECT')).click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null && ${editorText}.startsWith('GET /e2e_nav_orders/_search')`, 15_000);
  assert.match(await page.eval(editorText), /"query":\{"match_all":\{\}\}/);
  assert.equal(await runStatement(page), "3 rows");

  // A double click opens the documents: _id first, then the fields of the mapping.
  await openIndex(page, "e2e_nav_orders", "3 rows");
  const [header, ...rows] = table(await gridTsv(page));
  assert.equal(header[0], "_id");
  for (const column of ["title", "customer", "qty", "price", "ok", "tags"]) assert.ok(header.includes(column), `${column} in ${header}`);
  assert.deepEqual(rows.map((r) => r[0]).sort(), ["o1", "o2", "o3"]);
  const o1 = rows.find((r) => r[0] === "o1");
  assert.equal(o1[header.indexOf("qty")], "3");
  assert.equal(o1[header.indexOf("price")], "9.5");
  assert.equal(o1[header.indexOf("ok")], "true");
  assert.equal(o1[header.indexOf("title")], "First order");
  // Objects and arrays are JSON text, in the order the document had them.
  assert.equal(JSON.parse(o1[header.indexOf("customer")].replace(/^"|"$/g, "").replace(/""/g, '"')).address.zip, "00100");
  assert.equal(o1[header.indexOf("tags")].replace(/"/g, ""), "[a,b]");
  // A document without a field has a NULL there.
  const o3 = rows.find((r) => r[0] === "o3");
  assert.equal(o3[header.indexOf("tags")], "");

  // The document panel unfolds the focused row as a tree.
  const at = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  await page.click(at.x + 60, at.y + 40);
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.document-panel .doc-key')].some(k => k.textContent === 'customer')`, 10_000);

  // An index pattern in the connection narrows the list to the indices it names.
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("e2e_nav_orders")} === undefined`, 10_000);
  await expandCluster(page, NARROW, "e2e_nav_orders");
  await page.sleep(500);
  for (const other of ["e2e_nav_other", "e2e_nav_closed", ".e2e_nav_system"]) {
    assert.ok(await page.eval(`${treeRow(other)} === undefined`), `${other} listed under the pattern`);
  }
  await cleanup();
}
