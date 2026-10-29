// What the Elasticsearch scenarios (scenarios/search-*.mjs) share: the Elasticsearch of the
// `search` profile of docker/compose.yml (elastic / platypus, port 59200), a small HTTP client
// to seed and check indices behind the app's back, the saved connections, and the steps of the
// data viewer's grid editing.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { editorText, status, treeRow, visibleTab } from "./qa-sql.mjs";

export const PORT = 59200;
/** The OpenSearch of the same profile (no login). */
export const OPENSEARCH_PORT = 59201;
export const USER = "elastic";
export const PASSWORD = "platypus";

const BASE = `http://127.0.0.1:${PORT}`;
const AUTH = `Basic ${Buffer.from(`${USER}:${PASSWORD}`).toString("base64")}`;

/** A reason to skip when the cluster is not running. */
export async function skip() {
  try {
    const r = await fetch(`${BASE}/`, { headers: { authorization: AUTH }, signal: AbortSignal.timeout(2000) });
    if (r.ok) return null;
    return `Elasticsearch on port ${PORT} answers ${r.status}`;
  } catch {
    return `no Elasticsearch on port ${PORT} (docker compose -f docker/compose.yml --profile search up -d --wait elasticsearch)`;
  }
}

/** Whether the OpenSearch of the profile is running. */
export async function openSearchUp() {
  try {
    const r = await fetch(`http://127.0.0.1:${OPENSEARCH_PORT}/`, { signal: AbortSignal.timeout(2000) });
    return r.ok;
  } catch {
    return false;
  }
}

/** One request to the cluster: the parsed JSON reply (`null` when empty). An error status
 *  throws, unless it is one of `allow`. `body` is an object, or a string sent as NDJSON. */
export async function es(method, path, body = undefined, { allow = [] } = {}) {
  const ndjson = typeof body === "string";
  const r = await fetch(`${BASE}${path}`, {
    method,
    headers: { authorization: AUTH, ...(body === undefined ? {} : { "content-type": ndjson ? "application/x-ndjson" : "application/json" }) },
    body: body === undefined ? undefined : ndjson ? body : JSON.stringify(body),
  });
  const text = await r.text();
  if (!r.ok && !allow.includes(r.status)) throw new Error(`${method} ${path}: ${r.status} ${text}`);
  return text ? JSON.parse(text) : null;
}

/** Deletes the indices (and what the names match), ignoring those that are not there. */
export async function drop(...indices) {
  for (const index of indices) await es("DELETE", `/${index}`, undefined, { allow: [404] });
}

/** Creates `index` with `mapping` (a `mappings` object) and `settings`, and fills it with
 *  `docs`: objects, or `[id, object]` pairs. One shard, so that documents come back in the
 *  order they were written. Refreshed. */
export async function seed(index, mapping, docs, settings = {}) {
  await drop(index);
  await es("PUT", `/${index}`, { settings: { number_of_shards: 1, number_of_replicas: 0, ...settings }, mappings: mapping });
  await add(index, docs);
}

/** Adds documents (objects, or `[id, object]` pairs) to `index`, 2000 per request. */
export async function add(index, docs) {
  for (let from = 0; from < docs.length; from += 2000) {
    const lines = docs.slice(from, from + 2000).flatMap((d) => {
      const [id, doc] = Array.isArray(d) ? d : [undefined, d];
      return [JSON.stringify({ index: id === undefined ? {} : { _id: String(id) } }), JSON.stringify(doc)];
    });
    const reply = await es("POST", `/${index}/_bulk?refresh=true`, `${lines.join("\n")}\n`);
    if (reply.errors) throw new Error(`bulk into ${index}: ${JSON.stringify(reply.items.find((i) => i.index.error))}`);
  }
}

/** The `_source`s of the documents of `index` by `_id` (`{}` when the index is empty). */
export async function sources(index) {
  await es("POST", `/${index}/_refresh`);
  const reply = await es("POST", `/${index}/_search`, { size: 5000, query: { match_all: {} } });
  return Object.fromEntries(reply.hits.hits.map((h) => [h._id, h._source]));
}

/** A saved connection to the cluster. `extra` is more TOML lines; `fields` replaces the
 *  defaults (`user`, `password`: the command that prints the secret, `database`: the index
 *  pattern, `port`). */
export function connection(n, name, extra = "", fields = {}) {
  const f = { user: USER, password: PASSWORD, database: "", port: PORT, ...fields };
  return `[[connections]]
id = "00000000-0000-4000-8000-00000000e${String(n).padStart(3, "0")}"
name = "${name}"
driver = "elasticsearch"
host = "127.0.0.1"
port = ${f.port}
database = "${f.database}"
user = "${f.user}"
ssl_mode = "disable"
save_password = false
${f.password === null ? "" : `password_command = "echo ${f.password}"\n`}${extra}`;
}

export function saveConnections(dir, ...entries) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n\n${entries.join("\n")}`);
}

/** An API key of the cluster (the base64 `id:key` that `Authorization: ApiKey` takes). */
export async function apiKey(name) {
  return (await es("POST", "/_security/api_key", { name })).encoded;
}

export const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;

/** The text of the first line of every tree row label, for a quick look at the navigator. */
export const treeLabels = `[...document.querySelectorAll('.tree-row .tree-label')].map(l => l.textContent)`;

/** Expands the connection `name` and waits for the index `index` to be listed. */
export async function expandCluster(page, name, index) {
  await page.waitFor(`${treeRow(name)} !== undefined`, 30_000);
  await page.eval(`${treeRow(name)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow(index)} !== undefined`, 30_000);
}

/** Right-clicks the tree row `label` and returns the context menu's entries. */
export async function rowMenu(page, label) {
  await page.eval(`${treeRow(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  return page.eval(`[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`);
}

/** Closes an open context menu. */
export async function closeMenu(page) {
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}

/** Opens the data of the index `name` (a double click) and waits for `rows` ("3 rows"). */
export async function openIndex(page, name, rows, timeout = 20_000) {
  await page.waitFor(`${treeRow(name)} !== undefined`, 20_000);
  await page.eval(`${treeRow(name)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${editorText}.startsWith('GET /${name}/_search')`, 15_000);
  await page.waitFor(`${status} === ${JSON.stringify(rows)}`, timeout);
}

/** The shown grid as rows of cells, the header first, from the copy with headers. */
export function table(tsv) {
  return tsv.split("\n").map((line) => line.split("\t"));
}

/** Focuses the grid and moves to the cell at `row`, `col` (from the top-left one). */
async function goTo(page, row, col) {
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, 2);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

/** Types `value` into the cell at `row`, `col` of the grid and confirms it. */
export async function editCell(page, row, col, value) {
  await goTo(page, row, col);
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor') !== null`);
  await page.eval(`document.querySelector('.cell-editor').select?.()`);
  await page.key("a", "KeyA", 65, 2);
  if (value) await page.type(value);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
}

/** Marks the row at `row` for deletion. */
export async function deleteRow(page, row) {
  await goTo(page, row, 0);
  await page.key("Delete", "Delete", 46);
}

/** Adds a row on top of the grid. */
export async function insertRow(page) {
  await page.eval(`${grid}.focus()`);
  await page.key("Insert", "Insert", 45);
  await page.waitFor(`/row[s]? changed/.test(${editBar})`, 10_000);
}

/** Opens the preview of the changes: its text, or `null` when the app refuses it. */
export async function preview(page) {
  await dismissError(page);
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Preview SQL"]').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null || /needs|cannot|can not|invalid|no _id|not a/i.test(${shownError})`, 10_000);
  return page.eval(`document.querySelector('.sql-preview')?.textContent ?? null`);
}

/** Closes the preview without saving. */
export async function closePreview(page) {
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Close').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
}

/** Saves from the open preview and waits for the grid to reload with `rows`. */
export async function save(page, rows) {
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  await page.waitFor(`${editBar}.includes('to edit a cell') && ${status} === ${JSON.stringify(rows)}`, 20_000);
}

/** Everything the app shows as an error: toasts, notices and the edit bar. */
export const shownError = `[...document.querySelectorAll('.toast, .notice, .error, .edit-bar, .edit-error, .modal .error-text')].map(e => e.textContent).join(' | ')`;

/** Dismisses the edit bar's error, so that the next one cannot be mistaken for it. */
export async function dismissError(page) {
  await page.eval(`${visibleTab}.querySelector('.edit-error button')?.click()`);
  await page.waitFor(`${visibleTab}.querySelector('.edit-error') === null`, 5_000);
}

/** Discards the pending changes of the grid. */
export async function discard(page) {
  await page.eval(`[...${visibleTab}.querySelectorAll('.edit-bar button')].find(b => /Discard/.test(b.textContent + (b.getAttribute('aria-label') ?? '') + (b.title ?? ''))).click()`);
  await page.waitFor(`!/row[s]? changed/.test(${editBar})`, 10_000);
  await dismissError(page);
}
