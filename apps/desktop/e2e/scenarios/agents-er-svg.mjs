// Agents, ER diagram as SVG: get_er_diagram with format "svg" reads the catalog in the MCP server
// and has the open app draw it (the code of the diagram's own export). The app opens no
// connection, shows no tab or dialog and asks the user nothing; the answer is an image whose
// names are escaped (a table called <script>alert(1)</script>, a column with a quote and an
// ampersand), too big an image is refused rather than cut, an unknown format is refused naming
// the two, a connection closed to agents is refused as for Mermaid, and with the app closed the
// error says the SVG needs it and that Mermaid works without.
import assert from "node:assert/strict";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";

const SHOP = "00000000-0000-4000-8000-000000000061";
const BIG = "00000000-0000-4000-8000-000000000062";
const SECRET = "00000000-0000-4000-8000-000000000063";

const HOSTILE = "<script>alert(1)</script>";

export function skip() {
  return noMcp();
}

export function setup({ dir }) {
  const open = (file, sql) => {
    const db = new DatabaseSync(join(dir, file));
    db.exec(sql);
    db.close();
  };
  open(
    "shop.db",
    `CREATE TABLE customer (id INTEGER PRIMARY KEY, name TEXT);
     CREATE TABLE "${HOSTILE}" (id INTEGER PRIMARY KEY, "a""&b" TEXT NOT NULL, "<img src=x onerror=alert(1)>" TEXT,
       customer_id INTEGER REFERENCES customer (id));
     CREATE TABLE lonely (id INTEGER PRIMARY KEY);`,
  );
  // 40 tables of 12 columns: a picture of all of them is far over the byte limit.
  const columns = Array.from({ length: 12 }, (_, i) => `column_number_${i} INTEGER`).join(", ");
  open("big.db", Array.from({ length: 40 }, (_, i) => `CREATE TABLE big_${String(i).padStart(2, "0")} (id INTEGER PRIMARY KEY, ${columns});`).join("\n"));
  open("secret.db", "CREATE TABLE classified (id INTEGER PRIMARY KEY);");
  const entry = (id, name, file, access) =>
    `\n[[connections]]\nid = "${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${join(dir, file)}'\nagent_access = "${access}"\nsave_password = false\n`;
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${entry(SHOP, "Shop", "shop.db", "read_only")}${entry(BIG, "Big", "big.db", "read_only")}${entry(SECRET, "Secret", "secret.db", "off")}`);
}

/** The bridge's file, once the app wrote it. */
async function bridgeReady(page, dir) {
  const file = join(dir, "agent-bridge.json");
  for (let i = 0; i < 100 && !existsSync(file); i++) await page.sleep(100);
  assert.ok(existsSync(file), "the app wrote agent-bridge.json");
}

/** What the page shows that a request must not change. */
const pageState = (page) =>
  page.eval(`JSON.stringify({
    tabs: document.querySelectorAll('.query-tab, .erd-canvas, [data-tab-kind]').length,
    scripts: document.querySelectorAll('script').length,
    dialogs: document.querySelectorAll('.modal-backdrop, .agent-context, .agent-write, [role=dialog]').length,
    alerts: (window.__alerts ?? []).length,
    tables: document.querySelectorAll('.erd-table').length,
  })`);

/** The image parsed apart from the page, as XML: what is in it. */
const inspect = (page, svg) =>
  page.eval(`(() => {
    const doc = new DOMParser().parseFromString(${JSON.stringify(svg)}, "image/svg+xml");
    const all = [...doc.querySelectorAll("*")];
    return JSON.stringify({
      wellFormed: doc.querySelector("parsererror") === null,
      root: doc.documentElement.localName,
      scripts: doc.querySelectorAll("script, foreignObject, iframe, image, a").length,
      handlers: all.flatMap((e) => [...e.attributes].map((a) => a.name)).filter((n) => /^on/i.test(n)).length,
      tables: doc.querySelectorAll(".erd-table").length,
      texts: [...doc.querySelectorAll("text")].map((t) => t.textContent),
    });
  })()`).then(JSON.parse);

async function hostileNamesAreText(page, mcp, before) {
  const r = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", format: "svg" });
  assert.equal(r.error, false, r.text);
  assert.ok(r.text.startsWith("<svg"), r.text.slice(0, 80));
  assert.ok(!r.text.includes("<script"), "no script element in the text");
  assert.ok(r.text.includes("&lt;script&gt;alert(1)&lt;/script&gt;"), "the name is escaped");
  assert.ok(r.text.includes("a&quot;&amp;b"), "the quote and the ampersand are escaped");
  const seen = await inspect(page, r.text);
  assert.deepEqual([seen.wellFormed, seen.root, seen.scripts, seen.handlers], [true, "svg", 0, 0], JSON.stringify(seen));
  assert.equal(seen.tables, 3, "customer, the hostile table and lonely");
  assert.ok(seen.texts.includes(HOSTILE), `the name is text: ${seen.texts}`);
  assert.ok(seen.texts.some((t) => t.startsWith('a"&b')), "the column keeps its quote and ampersand");
  assert.ok(seen.texts.includes("PK") && seen.texts.includes("FK"), "keys are marked");
  // Nothing of it reached the app's page: no tab, no dialog, no script, no alert.
  assert.equal(await pageState(page), before, "the app showed nothing");
}

async function otherFormats(page, mcp, before) {
  const mermaid = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main" });
  assert.equal(mermaid.error, false, mermaid.text);
  assert.ok(mermaid.text.startsWith("erDiagram"), "Mermaid is still the default");
  const named = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", format: "mermaid" });
  assert.equal(named.text, mermaid.text);

  for (const format of ["png", "", "SVG;"]) {
    const bad = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", format });
    assert.ok(bad.error && /`mermaid`/.test(bad.text) && /`svg`/.test(bad.text), `${format}: ${bad.text}`);
  }
  // Case does not matter.
  const upper = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", format: "SVG" });
  assert.ok(!upper.error && upper.text.startsWith("<svg"), upper.text.slice(0, 120));
  assert.equal(await pageState(page), before);
}

async function chosenTables(page, mcp) {
  // One table and no hops: one box. With a hop, the table it points at comes too.
  const alone = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", tables: [HOSTILE], depth: 0, format: "svg" });
  assert.equal(alone.error, false, alone.text);
  assert.equal((await inspect(page, alone.text)).tables, 1);
  const near = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", tables: [HOSTILE], format: "svg" });
  const seen = await inspect(page, near.text);
  assert.equal(seen.tables, 2, "the hostile table and customer");
  assert.ok(seen.texts.includes("customer") && !seen.texts.includes("lonely"));
  // The same checks as for Mermaid: a table that is not there, a schema, a connection.
  const missing = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", tables: ["nope"], format: "svg" });
  assert.ok(missing.error && /no table `main.nope`/.test(missing.text), missing.text);
  const closed = await mcp.call("get_er_diagram", { connection: "Secret", schema: "main", format: "svg" });
  assert.ok(closed.error && /available to agents/.test(closed.text), closed.text);
  assert.ok(!closed.text.includes("classified"));
}

async function tooBig(mcp) {
  // An image is never cut: all 40 tables are refused, with the way out.
  const refused = await mcp.call("get_er_diagram", { connection: "Big", schema: "main", format: "svg" });
  assert.ok(refused.error, refused.text.slice(0, 200));
  assert.match(refused.text, /cannot be cut/);
  assert.match(refused.text, /`tables`/);
  assert.match(refused.text, /mermaid/);
  assert.ok(!refused.text.includes("<svg"), "no part of the image");
  // Mermaid is cut with a note instead, and a few tables fit.
  const cut = await mcp.call("get_er_diagram", { connection: "Big", schema: "main" });
  assert.ok(!cut.error && /%% cut: /.test(cut.text), cut.text.slice(0, 200));
  const few = await mcp.call("get_er_diagram", { connection: "Big", schema: "main", tables: ["big_07"], depth: 0, format: "svg" });
  assert.equal(few.error, false, few.text.slice(0, 200));
  assert.ok(few.text.length < 32 * 1024);
}

async function appClosed(mcp, dir) {
  // The app's bridge gone, as when it is closed: the SVG cannot be drawn, Mermaid can.
  const file = join(dir, "agent-bridge.json");
  const saved = readFileSync(file, "utf8");
  rmSync(file);
  try {
    const r = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", format: "svg" });
    assert.ok(r.error, r.text);
    assert.match(r.text, /needs it open/);
    assert.match(r.text, /not open/);
    assert.match(r.text, /format: "mermaid"/);
    const text = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", format: "mermaid" });
    assert.equal(text.error, false, text.text);
  } finally {
    writeFileSync(file, saved);
  }
}

export default async function (page, { dir }) {
  await bridgeReady(page, dir);
  await page.waitFor(`document.querySelector('.sidebar-header') !== null`);
  // Any alert the names could raise is recorded instead of shown.
  await page.eval(`window.__alerts = []; window.alert = (m) => window.__alerts.push(String(m));`);
  const before = await pageState(page);
  const mcp = await startMcp(dir, "e2e-er-svg");
  try {
    await hostileNamesAreText(page, mcp, before);
    await otherFormats(page, mcp, before);
    await chosenTables(page, mcp);
    await tooBig(mcp);
    // The app answers again after a request it refused.
    const again = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", format: "svg" });
    assert.equal(again.error, false, again.text);
    await appClosed(mcp, dir);
    const back = await mcp.call("get_er_diagram", { connection: "Shop", schema: "main", format: "svg" });
    assert.equal(back.error, false, `the bridge file is back: ${back.text.slice(0, 200)}`);
    assert.equal(await pageState(page), before, "still nothing shown");
  } finally {
    mcp.close();
  }
}
