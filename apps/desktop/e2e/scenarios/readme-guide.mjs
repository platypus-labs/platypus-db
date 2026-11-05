// The user guide's screenshots on the throwaway Shop dev SQLite data: the welcome screen, the
// network part of the connection form with an SSH tunnel, a binary cell that is a file, the
// import wizard's preview, and the Safety, Agents > Security and Diagnostics pages of the
// Settings. Runs only with PLATYPUS_README_SHOTS set to the folder the README's pictures go
// to (docs/screenshots); these land in its guide/ folder:
//
//   PLATYPUS_README_SHOTS=../../docs/screenshots npm run e2e -- readme-guide
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { revealItem } from "../menu-sub.mjs";
import { fill, formField } from "../conn-helpers.mjs";
import { NAME, grid, openProject, openTable, prepare, setup as shopSetup, skip, treeRow } from "./readme-sqlite.mjs";

export { skip };

const CSV = "price-list.csv";
const CSV_ROWS = [
  ["Espresso machine", 399],
  ["Grinder", 134.9],
  ["Milk jug", 19.5],
  ["Coffee beans 1 kg", 25],
  ["Descaler", 13.5],
  ["Tamper", 36],
];

/** `n` bytes after `head`, a pattern that is no magic number. */
const filled = (head, n) =>
  Buffer.concat([Buffer.from(head, "latin1"), Buffer.from(Array.from({ length: n - head.length }, (_, i) => (i * 7 + 3) % 251))]);
const DOCUMENTS = [
  ["Invoice 2941", filled("%PDF-1.7\n", 2_411_724), "2026-10-01"],
  ["Delivery note 2941", filled("%PDF-1.4\n", 188_416), "2026-10-01"],
  ["Warranty card", filled("%PDF-1.7\n", 96_256), "2026-10-02"],
  ["Product photo", filled("\x89PNG\r\n\x1a\n", 52_224), "2026-10-02"],
];

export function setup(ctx) {
  const { dir } = ctx;
  shopSetup(ctx);
  const db = new DatabaseSync(join(dir, "shop.db"));
  db.exec("CREATE TABLE documents (id INTEGER PRIMARY KEY, title TEXT NOT NULL, file BLOB, uploaded_at TEXT)");
  const insert = db.prepare("INSERT INTO documents VALUES (?, ?, ?, ?)");
  DOCUMENTS.forEach(([title, file, at], i) => insert.run(i + 1, title, file, at));
  db.close();
  writeFileSync(join(dir, CSV), `name,price\n${CSV_ROWS.map(([n, p]) => `${n},${p}`).join("\n")}\n`);
  // The connection is open to agents, read only, for Settings > Agents > Security.
  const file = join(dir, "connections.toml");
  // Agents' changes wait for an answer, as the app ships: the end-to-end runs turn that off.
  writeFileSync(join(dir, "settings.toml"), `[appearance]\ntheme = "platypus_dark"\n\n[agents]\nconfirm_writes = true\n`);
  writeFileSync(file, readFileSync(file, "utf8").replace('connection_type = "dev"', 'connection_type = "dev"\nagent_access = "read_only"'));
}

const toasts = `[...document.querySelectorAll('.toaster .toast')].map(t => t.textContent)`;
const dialog = `document.querySelector('.modal.import-data')`;
const settings = `document.querySelector('.modal.settings')`;

/** Nothing on screen may be an error: no toast and no notice that reports one. */
async function assertNoError(page) {
  const found = await page.eval(
    `[...document.querySelectorAll('.toaster .toast, .notice.error, .error')].map(e => e.textContent)`,
  );
  assert.deepEqual(found, [], `errors on screen: ${found}`);
}

/** Takes the focus off any field, checks that nothing on screen is an error, and lets it settle. */
async function settle(page) {
  await page.eval(`document.activeElement?.blur()`);
  await assertNoError(page);
  await page.sleep(300);
}

async function welcome(page, shot) {
  await page.waitFor(`document.querySelector('.welcome')?.textContent.includes('Try the sample database')`, 15_000);
  await settle(page);
  await shot("welcome");
}

/** The connection form for PostgreSQL on its Network tab with an SSH tunnel filled in. */
async function connectNetwork(page, shot) {
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  await page.waitFor(`document.querySelector('.driver-grid') !== null`);
  await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent.includes('PostgreSQL')).click()`);
  await page.waitFor(`document.querySelector('.modal .dialog-tabs [role=tab]') !== null`);
  const network = `[...document.querySelectorAll('.modal .dialog-tabs [role=tab]')].find(t => t.textContent.startsWith('Network'))`;
  await page.eval(`${network}.click()`);
  await page.waitFor(`${network}.getAttribute('aria-selected') === 'true'`);
  const tunnel = `[...document.querySelectorAll('.modal label')].find(l => l.textContent.includes('Connect through an SSH tunnel'))`;
  await page.waitFor(`${tunnel} !== undefined`);
  assert.equal(await page.eval(`document.querySelector('.modal .ssh-section input') === null`), true, "the tunnel starts off");
  await page.eval(`${tunnel}.querySelector('input').click()`);
  await page.waitFor(`document.querySelector('.modal .ssh-section input[placeholder="bastion.example.com"]') !== null`);
  await page.eval(fill(formField("SSH host", ".modal"), "bastion.example.com"));
  await page.eval(fill(formField("SSH user", ".modal"), "deploy"));
  await page.waitFor(
    `${formField("SSH host", ".modal")}.value === 'bastion.example.com' && ${formField("SSH user", ".modal")}.value === 'deploy'`,
  );
  await settle(page);
  await shot("connect-network");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal') === null`);
}

/** The grid of the documents table with its menu open on a PDF cell. */
async function fileCell(page, shot) {
  await page.eval(`(() => {
    window.__drawn = new Set();
    const original = CanvasRenderingContext2D.prototype.fillText;
    window.__fillText = original;
    CanvasRenderingContext2D.prototype.fillText = function (text, ...rest) { window.__drawn.add(String(text)); return original.call(this, text, ...rest); };
  })()`);
  await openTable(page, "documents");
  await page.waitFor(`[...window.__drawn].some(t => /^PDF · 2\\.3 MB$/.test(t))`, 15_000);
  const drawn = await page.eval(`[...window.__drawn]`);
  await page.eval(`CanvasRenderingContext2D.prototype.fillText = window.__fillText`);
  assert.ok(drawn.some((t) => /^PNG · 51(\.\d)? KB$/.test(t)), `a PNG label in ${drawn.filter((t) => t.includes("·"))}`);
  // The file cell of the first row, right-clicked towards the right of its column so that the
  // menu leaves the column in view.
  await page.eval(`${grid}.id = 'e2e-grid'`);
  const r = await page.rect("#e2e-grid");
  await page.eval(`document.getElementById('e2e-grid').removeAttribute('id')`);
  const left = r.x + 338;
  await page.click(left, r.y + 52, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  const items = await page.eval(`[...document.querySelectorAll('.context-menu button')].map(b => b.querySelector('span')?.textContent)`);
  assert.ok(items.includes("Save PDF…"), `Save PDF… in ${items}`);
  await assertNoError(page);
  await page.sleep(300);
  await shot("file-cell");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
  // The tab goes, so that the wizard shows over the welcome page.
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`document.querySelectorAll('.tab').length === 0`);
}

/** The import wizard reading a CSV into the products table. */
async function importPreview(page, dir, shot) {
  await page.eval(`${treeRow("products")}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  assert.ok(await revealItem(page, "Import data"), "the menu offers Import data");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Import data')).click()`);
  await page.waitFor(`${dialog} !== null`);
  const path = join(dir, CSV);
  await page.eval(fill(`${dialog}.querySelector('input')`, path));
  await page.waitFor(`${dialog}.querySelector('input').value === ${JSON.stringify(path)}`);
  await page.eval(`${dialog}.querySelector('input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))`);
  await page.waitFor(`${dialog}.querySelectorAll('.import-preview tbody tr').length === ${CSV_ROWS.length}`, 15_000);
  const preview = await page.eval(`${dialog}.querySelector('.import-preview').textContent`);
  assert.ok(preview.includes("Espresso machine") && preview.includes("Tamper"), preview);
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.import-mapping select').length`), 3, "every column of products has a mapping");
  // The file's folder is a temporary one of this computer: the picture shows another.
  await page.eval(`${dialog}.querySelector('input').value = ${JSON.stringify(["C:", "Shop", CSV].join("\\"))}`);
  await settle(page);
  await shot("import-preview");
  await page.eval(`${dialog}.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))`);
  await page.waitFor(`${dialog} === null`);
}

async function settingsPages(page, shot) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`${settings} !== null`);

  await page.settingsTab("Safety");
  await settle(page);
  await shot("safety");

  await page.settingsTab("Agents");
  await page.waitFor(`document.querySelector('[data-agents-page="security"]') !== null`);
  await page.eval(`document.querySelector('[data-agents-page="security"]').click()`);
  const row = `document.querySelector('.agents-connections tr[data-connection=${JSON.stringify(NAME)}]')`;
  await page.waitFor(`${row} !== null && ${row}.closest('[hidden]') === null`);
  assert.equal(await page.eval(`${row}.querySelector('td:last-child').textContent`), "Read only");
  await settle(page);
  await shot("agents-security");

  await page.settingsTab("Diagnostics");
  const field = `document.querySelector('.diagnostics-panel [data-field="log_dir"]')`;
  await page.waitFor(`${field} !== null`);
  // The folder is under this computer's user name: the picture shows the installed app's instead.
  await page.eval(`${field}.textContent = '%APPDATA%\\\\dev.platypusdb.desktop\\\\logs'`);
  assert.ok(await page.eval(`document.querySelector('[data-action="save-diagnostics"]') !== null`));
  const text = await page.eval(`document.querySelector('.modal.settings').textContent`);
  assert.ok(!/Users[\\/]/i.test(text), "no user folder on the page");
  await settle(page);
  await shot("diagnostics");

  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`${settings} === null`);
}

export default async function (page, { dir }) {
  const out = join(resolve(process.env.PLATYPUS_README_SHOTS), "guide");
  mkdirSync(out, { recursive: true });
  const shot = (name) => page.screenshot(join(out, `${name}.png`));
  await prepare(page);
  // The version and the build's commit and time would change every picture over the welcome page at each run.
  await page.eval(`document.head.append(Object.assign(document.createElement('style'), { textContent: '.welcome-cta + p { visibility: hidden }' }))`);

  await welcome(page, shot);
  await connectNetwork(page, shot);

  await openProject(page, 2, "Shop");
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("documents")} !== undefined && ${treeRow("products")} !== undefined`, 15_000);
  await fileCell(page, shot);
  await importPreview(page, dir, shot);
  await settingsPages(page, shot);
  assert.deepEqual(await page.eval(toasts), []);
}
