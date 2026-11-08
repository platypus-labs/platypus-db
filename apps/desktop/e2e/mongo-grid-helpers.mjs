// What the MongoDB grid-editing and transaction scenarios (scenarios/mongo-grid-edit*.mjs,
// scenarios/mongo-transactions.mjs) share: the standalone server (port 57017) and the
// single-node replica set (port 57018) of the `mongo` profile of docker/compose.yml, a
// mongosh to seed and check collections behind the app's back, the saved connections (the
// password comes from a command, so there is no prompt) and the steps of editing a result in
// the grid.
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { spawnSync } from "./proc.mjs";
import { CTRL } from "./cdp.mjs";
import { editorText, gridTsv, runScript, status, treeRow, visibleTab } from "./qa-sql.mjs";
import { openData } from "./qa-grid.mjs";
import { editBar, shownError, table } from "./search-helpers.mjs";

export { deleteRow, discard, dismissError, editBar, editCell, insertRow, save, shownError, table } from "./search-helpers.mjs";
export { openEditor, runScript, runStatement } from "./qa-sql.mjs";
export { expand, openData, treeMenu } from "./qa-grid.mjs";

export const SERVERS = {
  standalone: {
    label: "the standalone MongoDB",
    port: 57017,
    shell: ["exec", "platypus-db-mongo-1", "mongosh", "--quiet", "-u", "platypus", "-p", "platypus", "--authenticationDatabase", "admin", "platypus"],
  },
  replica: {
    label: "the MongoDB replica set",
    port: 57018,
    shell: ["exec", "platypus-db-mongo-rs-1", "mongosh", "--port", "57018", "--quiet", "platypus"],
  },
};

/** A `skip()` for scenarios that need these servers (`standalone`, `replica`). */
export const needs =
  (...servers) =>
  async () => {
    for (const name of servers) {
      const { port, label } = SERVERS[name];
      const up = await new Promise((resolve) => {
        const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
        const done = (ok) => {
          socket.destroy();
          resolve(ok);
        };
        socket.on("connect", () => done(true));
        socket.on("error", () => done(false));
        socket.on("timeout", () => done(false));
      });
      if (!up) return `no ${label} on port ${port} (docker compose -f docker/compose.yml --profile mongo up -d --wait mongo mongo-rs)`;
    }
    return null;
  };

/** Runs a mongosh script on `server`, in the database `platypus`; returns what it printed. The script
 *  goes in `--eval`: read from stdin, mongosh echoes its prompt into the output. */
export function mongosh(server, script) {
  const r = spawnSync("docker", [...SERVERS[server].shell, "--eval", script], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${server}: ${r.stderr || r.stdout}`);
  return r.stdout.replace(/\r/g, "").trim();
}

/** How many documents of `collection` match `filter` (mongosh text, `{}` for all). */
export function count(server, collection, filter = "{}") {
  // mongosh prints its deprecation warnings (a number in `NumberLong`) before the result.
  const out = mongosh(server, `print(db.getCollection(${JSON.stringify(collection)}).countDocuments(${filter}))`);
  return Number(out.split("\n").at(-1));
}

/** A saved connection to `server`; `n` makes its id. `extra` is more TOML lines. */
export function connection(n, name, server, extra = "") {
  const replica = server === "replica";
  return `[[connections]]
id = "00000000-0000-4000-8000-00000000f${String(n).padStart(3, "0")}"
name = "${name}"
driver = "mongo"
host = "localhost"
port = ${SERVERS[server].port}
database = "${replica ? "platypus?replicaSet=rs0" : "platypus?authSource=admin"}"
${replica ? "" : 'user = "platypus"\n'}password_command = "echo platypus"
ssl_mode = "disable"
save_password = false
${extra}`;
}

export function saveConnections(dir, ...entries) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n\n${entries.join("\n")}`);
}

/** Expands the connection `name` and waits for the collection `child` to be listed. */
export async function expandConnection(page, name, child) {
  await page.waitFor(`${treeRow(name)} !== undefined`, 30_000);
  await page.eval(`${treeRow(name)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow(child)} !== undefined`, 30_000);
}

/** Waits for the shown result to have `rows` ("3 rows") and the grid to be editable. */
export async function editable(page, rows) {
  await page.waitFor(`${status} === ${JSON.stringify(rows)} && ${editBar}.includes('to edit a cell')`, 20_000);
}

/** Runs the statement of the visible data viewer again, and waits for its `rows` and an
 *  editable grid. The status is cleared first: the old one could already say "3 rows". */
export async function rerun(page, rows) {
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await editable(page, rows);
}

/** Brings forward the data viewer of `label` and reads it again: the collection may have
 *  changed since (an undo script, a mongosh) while the tab showed the old documents. */
export async function openFresh(page, label, rows) {
  await openData(page, label);
  await rerun(page, rows);
}

/** Whether the result of the statement that just ran is editable: waits a moment for the edit
 *  bar, which shows once the table behind the result is known. */
export async function isEditable(page, wait = 3000) {
  try {
    await page.waitFor(`${editBar}.includes('to edit a cell')`, wait);
    return true;
  } catch {
    return false;
  }
}

/** The shown grid, the header first, and where a document (by its first cell) and a column are. */
export async function layout(page) {
  const [header, ...rows] = table(await gridTsv(page));
  return {
    header,
    rows,
    col: (name) => {
      if (!header.includes(name)) throw new Error(`no column ${name} in ${header}`);
      return header.indexOf(name);
    },
    row: (id) => {
      const at = rows.findIndex((r) => r[0] === String(id));
      if (at < 0) throw new Error(`no document ${id} in ${rows.map((r) => r[0])}`);
      return at;
    },
  };
}

/** Focuses the grid and moves to the cell at `row`, `col` (from the top-left one). */
async function goTo(page, row, col) {
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("Home", "Home", 36, 2);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

/** Sets the cell at `row`, `col` to NULL (Shift+Delete). */
export async function setNull(page, row, col) {
  await goTo(page, row, col);
  await page.key("Delete", "Delete", 46, 8);
}

/** Whether F2 on the cell opens an editor: false for a result the grid cannot edit. */
export async function opensEditor(page, row, col) {
  await goTo(page, row, col);
  await page.key("F2", "F2", 113);
  await page.sleep(400);
  const open = await page.eval(`document.querySelector('.cell-editor, .cell-popup-text, .list-editor') !== null`);
  if (open) await page.key("Escape", "Escape", 27);
  return open;
}

/** Opens the preview of the changes: `{ sql, error }`, the text of the preview, or the error
 *  the app showed instead. */
export async function previewOrError(page) {
  await page.eval(`${visibleTab}.querySelector('.edit-error button')?.click()`);
  await page.eval(`${visibleTab}.querySelector('.edit-bar button[aria-label="Preview SQL"]').click()`);
  await page.waitFor(
    `document.querySelector('.sql-preview') !== null || ${visibleTab}.querySelector('.edit-error') !== null || /cannot|invalid|not a |several types|path or an operator|does not fit/i.test(${shownError})`,
    10_000,
  );
  return {
    sql: await page.eval(`document.querySelector('.sql-preview')?.textContent ?? null`),
    error: await page.eval(shownError),
  };
}

/** Closes the open preview without saving. */
export async function closePreview(page) {
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.sql-preview') === null`, 5_000);
}

/** The statements of the preview, one per line. */
export async function preview(page) {
  const { sql, error } = await previewOrError(page);
  if (sql === null) throw new Error(`no preview: ${error}`);
  return sql;
}

/** Saves from the open preview and waits for an error that matches `re`; returns the text shown. */
export async function saveFails(page, re) {
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`${re}.test(${shownError})`, 15_000);
  return page.eval(shownError);
}

/** Opens the undo script the app offers after a save, in its own tab; returns its text. */
export async function openUndo(page) {
  await page.waitFor(`[...${visibleTab}.querySelectorAll('button')].some(b => b.textContent.includes('Undo script'))`, 15_000);
  const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`[...${visibleTab}.querySelectorAll('button')].find(b => b.textContent.includes('Undo script')).click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${before}`, 10_000);
  await page.waitFor(`${editorText}.length > 0`, 10_000);
  // The script ends with a newline.
  return (await page.eval(editorText)).trimEnd();
}

/** Runs the undo script of the last save: opens it, runs it as a script, returns its text. */
export async function runUndo(page) {
  const text = await openUndo(page);
  await runScript(page);
  return text;
}

/** The button of the visible tab's toolbar with this label. */
export const toolbarButton = (label) =>
  `${visibleTab}.querySelector('.toolbar button[aria-label=${JSON.stringify(label)}]')`;
