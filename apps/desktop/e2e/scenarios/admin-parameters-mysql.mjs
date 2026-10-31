// Group S against the dev MySQL 8 and MariaDB: Server activity's Parameters view (the global
// variables). MySQL tells where each value comes from (performance_schema.variables_info)
// and no default; MariaDB tells the default and whether a variable is read-only. A variable
// is changed with SET GLOBAL as root for the run, so the view has one that differs: the list
// is sorted and cut at 500 rows with a line saying so, the search keeps the variables that
// have every word, "Only changed" keeps the changed ones (not max_connections), the details
// show the value, the source or default and range, and the SET GLOBAL statement opens in the
// editor without running; run there, the server refuses a read-only variable (datadir) and
// the value stays. MariaDB lists a read-only variable without a statement and says so.
// See the Parameters component in src/admin/Parameters.tsx.
import { spawnSync } from "../proc.mjs";
import assert from "node:assert/strict";
import { CTRL, clearStatus, connections, idle, needs, visibleTab } from "../qa-grid.mjs";
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

export const skip = needs("mysql", "mariadb");

const VARIABLE = "sort_buffer_size";

/** Runs `sql` as root in the container of `db`. */
function root(db, sql) {
  const client = db === "mariadb" ? "mariadb" : "mysql";
  const r = spawnSync("docker", ["exec", "-i", `platypus-db-${db}-1`, client, "-uroot", "-pplatypus", "-N", "-B"], {
    input: sql,
    encoding: "utf8",
  });
  if (r.status !== 0) throw new Error(`${db} root: ${r.stderr || r.stdout}`);
  return r.stdout.replace(/\r/g, "").trim();
}

export function setup({ dir }) {
  connections(dir, [["QA S MySQL parameters", "mysql"], ["QA S MariaDB parameters", "mariadb"]]);
  for (const db of ["mysql", "mariadb"]) root(db, `SET GLOBAL ${VARIABLE} = 524288;`);
}

const rows = (page) => page.eval(`${adminTab}.querySelectorAll('table.admin-parameters tbody tr').length`);
const countText = (page) => page.eval(`${adminTab}.querySelector('[data-field="parameter-count"]').textContent`);
const onlyChanged = `${adminTab}.querySelector('.admin-toolbar input[type=checkbox]')`;
const value = (db, name) => root(db, `SELECT @@global.${name};`);

/** Brings the Server activity tab of connection `name` back to the front. */
async function backToAdmin(page, name) {
  const title = JSON.stringify(`Server activity — ${name}`);
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.includes(${title})).click()`);
  await page.waitFor(`${adminTab} != null`, 15_000);
}

async function select(page, name) {
  await page.waitFor(`${parameterRow(name)} != null`, 15_000);
  await page.eval(`${parameterRow(name)}.click()`);
  await page.waitFor(`${adminTab}.querySelector('.admin-details-title')?.textContent === ${JSON.stringify(name)}`);
}

async function flow(page, db, name) {
  const maria = db === "mariadb";
  await openAdmin(page, name, db);
  await showSection(page, "Parameters");
  await page.waitFor(`${parameterRow(VARIABLE)} != null || ${adminTab}.querySelector('.admin-empty') != null`, 30_000);

  // Hundreds of variables: only the first 500 rows are drawn, and the line says how many more.
  assert.equal(await rows(page), 500);
  assert.match(await countText(page), /^\d{3,} parameters$/);
  assert.match(await page.eval(`${adminTab}.querySelector('.admin-empty')?.textContent ?? ''`), /\d+ more: narrow the search to see them\./);
  const names = await parameterNames(page);
  assert.deepEqual(names, [...names].sort(), "sorted by name");

  // The search: every word, in the name or the description; nothing found says so.
  await setSearch(page, VARIABLE);
  await page.waitFor(`${parameterRow(VARIABLE)} != null && ${parameterRow("max_connections")} == null`);
  assert.equal(await page.eval(`${adminTab}.querySelector('.admin-empty') != null`), false, "all rows fit: no line of more");
  assert.equal(await parameterCell(page, VARIABLE, 1), "524288");
  await setSearch(page, "SORT BUFFER");
  await page.waitFor(`${parameterRow(VARIABLE)} != null && ${parameterRow("join_buffer_size")} == null`);
  await setSearch(page, "no variable is called like this");
  await page.waitFor(`${adminTab}.querySelector('.admin-empty')?.textContent === 'No parameters match.'`);
  assert.equal(await rows(page), 0);
  assert.equal(await countText(page), "0 parameters");
  await setSearch(page, "");

  // Where it comes from, or its default: what each server tells.
  await setSearch(page, VARIABLE);
  await page.waitFor(`${parameterRow(VARIABLE)} != null`);
  if (maria) {
    assert.equal(await parameterCell(page, VARIABLE, 2), "2097152", "MariaDB tells the default");
    assert.equal(await parameterCell(page, VARIABLE, 3), "SQL", "set with SET GLOBAL");
    assert.equal(await parameterCell(page, VARIABLE, 4), "dynamic");
  } else {
    assert.equal(await parameterCell(page, VARIABLE, 2), "", "MySQL tells no default");
    assert.equal(await parameterCell(page, VARIABLE, 3), "DYNAMIC", "set with SET GLOBAL");
  }

  // Only the changed: the variable set above, not max_connections, and with a search too.
  await setSearch(page, "");
  await page.eval(`${onlyChanged}.click()`);
  await page.waitFor(`${parameterRow(VARIABLE)} != null`, 15_000);
  assert.equal(await page.eval(`${parameterRow("max_connections")} != null`), false, "max_connections is as built");
  assert.ok((await rows(page)) < 500);
  assert.ok(
    await page.eval(`[...${adminTab}.querySelectorAll('table.admin-parameters tbody tr')].every(r => r.classList.contains('changed'))`),
    "every row left is changed",
  );
  await setSearch(page, "zzz_nothing");
  await page.waitFor(`${adminTab}.querySelector('.admin-empty')?.textContent === 'No parameters match.'`);
  await setSearch(page, "");
  await page.eval(`${onlyChanged}.click()`);
  await page.waitFor(`${parameterRow(VARIABLE)} == null`, 15_000);
  assert.equal(await rows(page), 500);

  // The details and the statement, opened in the editor and not run.
  await setSearch(page, VARIABLE);
  await select(page, VARIABLE);
  assert.equal(await page.eval(field("value")), "524288");
  assert.match(await page.eval(field("limits")), maria ? /^1024 – / : /^32768 – /);
  assert.equal(
    await page.eval(`${adminTab}.querySelector('[data-field="parameter-statement"]').textContent`),
    `SET GLOBAL ${VARIABLE} = 524288;`,
  );
  const tabs = await page.eval(`document.querySelectorAll('.tab').length`);
  await page.eval(`${adminButton("Open in the editor")}.click()`);
  await page.waitFor(`document.querySelectorAll('.tab').length > ${tabs}`);
  await page.waitFor(`${editorText}.includes('SET GLOBAL ${VARIABLE} = 524288;')`, 15_000);
  assert.equal(value(db, VARIABLE), "524288", "nothing ran");

  // A variable the server only reads at start: MySQL offers the statement and the server
  // refuses it; MariaDB offers none and says why.
  await backToAdmin(page, name);
  await page.waitFor(`${adminTab} != null`, 15_000);
  await setSearch(page, "datadir");
  await select(page, "datadir");
  if (maria) {
    assert.equal(await page.eval(`${adminTab}.querySelector('[data-field="parameter-statement"]')?.textContent ?? null`), null);
    assert.match(await page.eval(`${adminTab}.querySelector('.admin-details').textContent`), /does not let this parameter be set/);
    assert.equal(await page.eval(field("context")), "Read only: it changes in the configuration, with a restart.");
  } else {
    assert.match(await page.eval(`${adminTab}.querySelector('[data-field="parameter-statement"]').textContent`), /^SET GLOBAL datadir = '/);
    await page.eval(`${adminButton("Open in the editor")}.click()`);
    await page.waitFor(`${editorText}.includes('SET GLOBAL datadir')`, 15_000);
    await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
    await clearStatus(page);
    await page.key("Enter", "Enter", 13, CTRL);
    await idle(page);
    const error = await page.eval(`${visibleTab}.querySelector('.result-error')?.textContent ?? ''`);
    assert.match(error, /read only variable|datadir/i);
    await backToAdmin(page, name);
    await page.waitFor(`${adminTab} != null`, 15_000);
  }

  // Reading again keeps the view.
  await page.eval(`${adminTab}.querySelector('button[aria-label="Refresh now"]').click()`);
  await page.waitFor(`${adminTab}.querySelector('button[aria-label="Refresh now"]').disabled === false`, 20_000);
  assert.equal(await page.eval(`${adminTab}.querySelector('[data-field="parameters-error"]') != null`), false);
  assert.equal(await page.eval(`${parameterRow("datadir")} != null`), true);
}

export default async function (page) {
  try {
    await flow(page, "mysql", "QA S MySQL parameters");
    await flow(page, "mariadb", "QA S MariaDB parameters");
  } finally {
    for (const db of ["mysql", "mariadb"]) root(db, `SET GLOBAL ${VARIABLE} = DEFAULT;`);
  }
}
