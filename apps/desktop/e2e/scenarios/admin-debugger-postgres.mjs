// Group J: the PL/pgSQL debugger, against the PostgreSQL with pldebugger of docker/compose.yml
// (`docker compose -f docker/compose.yml --profile pldebug up -d --build --wait`, on 55437).
// Debug… on a function first offers to create the missing pldbgapi extension; it explains why
// a SQL function and a trigger function cannot be debugged; a value the parameter's type
// refuses stays in the dialog. Then a call of a function with a loop: a breakpoint set from
// the gutter, F5 to it, the variables, F11 into the nested function, its stack and the
// caller's frame, F10 back, a variable changed (and a value refused), the breakpoint cleared
// and the call run to its result and messages. Then the ends: Stop (Shift+F5) while paused
// rolls back what the call wrote, Stop while a continue runs an endless loop, and closing the
// tab while paused; none leaves a session running.
import { DBS, SHIFT, assert, connections, expand, needs, sh, treeMenu } from "../qa-grid.mjs";
import { openPath } from "../qa-dbs.mjs";

const DB = "pldebug";
const NAME = "QA J PL/pgSQL debugger";

DBS[DB] = {
  port: 55437,
  password: "platypus",
  toml: `driver = "postgres"\nhost = "localhost"\nport = 55437\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`,
  shell: [
    "exec", "-i", "platypus-db-postgres-pldebug-1",
    "psql", "-U", "platypus", "-d", "platypus", "-v", "ON_ERROR_STOP=1", "-q", "-A", "-t", "-F", "\t",
  ],
};

const SUM = "e2e_dbg_sum(n integer, start integer)";
const DOUBLE = "e2e_dbg_double(x integer)";
const SPIN = "e2e_dbg_spin()";
const SQL_FN = "e2e_dbg_sql(x integer)";
const TRIGGER_FN = "e2e_dbg_trg()";
const FUNCTIONS = [NAME, "platypus", "public", "Functions"];

/** The loop of e2e_dbg_sum: the line of its INSERT, then the line the breakpoint goes on. */
const SUM_SOURCE = `
DECLARE
    total integer := start;
BEGIN
    FOR i IN 1..n LOOP
        INSERT INTO e2e_dbg_log VALUES (i);
        total := total + e2e_dbg_double(i);
    END LOOP;
    RAISE NOTICE 'total %', total;
    RETURN total;
END
`;
const lineOf = (text) => SUM_SOURCE.split("\n").findIndex((l) => l.includes(text)) + 1;
const INSERT_LINE = lineOf("INSERT INTO");
const BODY_LINE = lineOf("total := total +");

export async function skip() {
  const down = await needs(DB)();
  if (down) return "no PostgreSQL with the debugger on 55437 (docker compose -f docker/compose.yml --profile pldebug up -d --build --wait)";
  const has = sh(DB, "SELECT count(*) FROM pg_available_extensions WHERE name = 'pldbgapi';");
  return has === "1" ? null : "the PostgreSQL on 55437 has no pldbgapi";
}

export function setup({ dir }) {
  sh(
    DB,
    `DROP EXTENSION IF EXISTS pldbgapi;
     DROP TABLE IF EXISTS e2e_dbg_log;
     CREATE TABLE e2e_dbg_log (i integer);
     CREATE OR REPLACE FUNCTION e2e_dbg_double(x integer) RETURNS integer LANGUAGE plpgsql AS $$
BEGIN
    RETURN x * 2;
END
$$;
     CREATE OR REPLACE FUNCTION e2e_dbg_sum(n integer, start integer DEFAULT 0) RETURNS integer
     LANGUAGE plpgsql AS $$${SUM_SOURCE}$$;
     CREATE OR REPLACE FUNCTION e2e_dbg_spin() RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    LOOP
        PERFORM pg_sleep(0.05);
    END LOOP;
END
$$;
     CREATE OR REPLACE FUNCTION e2e_dbg_sql(x integer) RETURNS integer LANGUAGE sql AS 'SELECT x + 1';
     CREATE OR REPLACE FUNCTION e2e_dbg_trg() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RETURN NEW;
END
$$;`,
  );
  connections(dir, [[NAME, DB]]);
}

const dialog = `document.querySelector('.modal.debug-dialog')`;
const tab = `[...document.querySelectorAll('.debugger-tab')].find(t => t.style.display !== 'none')`;
const inTab = (selector) => `${tab}?.querySelector(${JSON.stringify(selector)})`;
const statusOf = `(${inTab('[data-field="debug-status"]')}?.dataset.state ?? '')`;
const where = `(${inTab('[data-field="debug-routine"]')}?.textContent ?? '')`;
const variable = (name) => `(${tab}?.querySelector('[data-field="debug-variables"] tr[data-name=${JSON.stringify(name)}] .debug-value')?.textContent.trim() ?? null)`;
const tabError = `(${inTab('[data-field="debug-error"]')}?.textContent ?? '')`;

/** Sets a React-controlled input or select of the page found by `element`. */
const setValue = (page, element, value) =>
  page.eval(`(() => {
    const e = ${element};
    const proto = e instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(e, ${JSON.stringify(value)});
    e.dispatchEvent(new Event(e instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  })()`);

/** The argument row of parameter `name` in the dialog. */
const argument = (name) => `${dialog}.querySelector('.debug-argument[data-param=${JSON.stringify(name)}]')`;

/** Debug… on routine `routine` of the navigator, and waits for the dialog to have read it. */
async function openDialog(page, routine) {
  await openPath(page, [...FUNCTIONS, routine]);
  await treeMenu(page, routine, "Debug…");
  await page.waitFor(`${dialog} !== null`);
  await page.waitFor(
    `${dialog}.querySelector('[data-field="debug-arguments"], [data-field="debug-unavailable"], [data-field="debug-error"]') !== null`,
    20_000,
  );
}

async function closeDialog(page) {
  await page.eval(`[...${dialog}.querySelectorAll('.actions button')].find(b => !b.classList.contains('primary')).click()`);
  await page.waitFor(`${dialog} === null`);
}

/** Starts the call of e2e_dbg_sum with `n` (`start` at its default) and waits for its tab,
 *  paused at its first statement. */
async function debugSum(page, n) {
  await openDialog(page, SUM);
  await setValue(page, `${argument("n")}.querySelector('input')`, n);
  await page.eval(`${dialog}.querySelector('[data-field="debug-start"]').click()`);
  await page.waitFor(`${dialog} === null && ${tab} !== undefined`, 30_000);
  await page.waitFor(`${statusOf} === 'stopped'`, 30_000);
}

/** A key of the debugger, on the tab. */
async function press(page, key, modifiers = 0) {
  const codes = { F5: 116, F10: 121, F11: 122 };
  await page.eval(`${tab}.focus()`);
  await page.key(key, key, codes[key], modifiers);
}

/** Clicks the breakpoint gutter beside line `n` of the source shown. */
async function clickGutter(page, n) {
  const at = await page.eval(`(() => {
    const line = [...${tab}.querySelectorAll('.debug-source .cm-line')][${n - 1}].getBoundingClientRect();
    const gutter = ${tab}.querySelector('.debug-source .debug-breakpoints').getBoundingClientRect();
    return { x: gutter.x + gutter.width / 2, y: line.y + line.height / 2 };
  })()`);
  await page.click(at.x, at.y);
}

/** The lines with a breakpoint dot in the gutter, as JSON. The gutter's hidden spacer holds a
 *  dot too (it sizes the gutter), so only the visible ones count. */
const breakpointLines = `JSON.stringify((() => {
  const t = ${tab};
  if (!t) return [];
  const lines = [...t.querySelectorAll('.debug-source .cm-line')].map(l => l.getBoundingClientRect());
  return [...t.querySelectorAll('.debug-breakpoint-dot')]
    .filter(d => d.closest('.cm-gutterElement')?.style.visibility !== 'hidden')
    .map(d => {
      const r = d.getBoundingClientRect();
      const y = r.top + r.height / 2;
      return lines.findIndex(l => y >= l.top && y < l.bottom) + 1;
    });
})())`;

/** No debugged call still runs on the server. */
async function noCallLeft(page) {
  const count = () =>
    sh(DB, "SELECT count(*) FROM pg_stat_activity WHERE state = 'active' AND query LIKE '%e2e_dbg_%' AND pid <> pg_backend_pid();");
  for (let i = 0; i < 50 && count() !== "0"; i++) await page.sleep(200);
  assert.equal(count(), "0", "a debugged call is still running");
}

async function cannotDebug(page, routine, text) {
  await openDialog(page, routine);
  await page.waitFor(`${dialog}.querySelector('[data-field="debug-unavailable"]')?.textContent.includes(${JSON.stringify(text)}) ?? false`);
  assert.equal(await page.eval(`${dialog}.querySelector('[data-field="debug-start"]').disabled`), true, routine);
  await closeDialog(page);
}

async function setupChecks(page) {
  await expand(page, NAME, DB);
  // The extension is missing: the dialog offers to create it, then shows the arguments.
  await openDialog(page, SUM);
  await page.waitFor(`${dialog}.querySelector('[data-field="debug-unavailable"]')?.textContent.includes('pldbgapi') ?? false`);
  assert.equal(await page.eval(`${dialog}.querySelector('[data-field="debug-start"]').disabled`), true);
  await page.eval(`${dialog}.querySelector('[data-field="debug-install"]').click()`);
  await page.waitFor(`${dialog}.querySelector('[data-field="debug-arguments"]') !== null`, 20_000);
  assert.equal(sh(DB, "SELECT count(*) FROM pg_extension WHERE extname = 'pldbgapi';"), "1");
  // `start` keeps its default; `n` has none to keep.
  assert.equal(await page.eval(`${argument("start")}.querySelector('select').value`), "default");
  assert.equal(await page.eval(`[...${argument("n")}.querySelectorAll('option')].some(o => o.value === 'default')`), false);
  // A value the type refuses: the error stays in the dialog, which stays open.
  await setValue(page, `${argument("n")}.querySelector('input')`, "three");
  await page.eval(`${dialog}.querySelector('[data-field="debug-start"]').click()`);
  await page.waitFor(`${dialog}.querySelector('[data-field="debug-error"]')?.textContent.includes('integer') ?? false`, 30_000);
  assert.equal(await page.eval(`${tab} === undefined`), true, "no tab for a call that did not start");
  await closeDialog(page);
  await noCallLeft(page);

  await cannotDebug(page, SQL_FN, "PL/pgSQL");
  await cannotDebug(page, TRIGGER_FN, "trigger");
}

/** The call of e2e_dbg_sum(3) stepped through and run to its result. */
async function debugToTheEnd(page) {
  await debugSum(page, "3");
  assert.match(await page.eval(where), /^e2e_dbg_sum/);
  assert.ok(await page.eval(`${inTab(".debug-current-line")} !== null`), "the current line is marked");
  assert.ok((await page.eval(`${inTab(".debug-source .cm-content")}.textContent`)).includes("FOR i IN 1..n LOOP"));

  // A breakpoint on the loop's body, then F5 to it.
  assert.equal(await page.eval(breakpointLines), "[]", "no breakpoint yet");
  await clickGutter(page, BODY_LINE);
  await page.waitFor(`${breakpointLines} === '[${BODY_LINE}]'`);
  await press(page, "F5");
  await page.waitFor(`${where}.endsWith('line ${BODY_LINE}')`, 20_000);
  await page.waitFor(`${variable("i")} === '1'`, 10_000);
  assert.equal(await page.eval(variable("n")), "3");
  assert.equal(await page.eval(variable("total")), "0");

  // F11 into the nested function: two frames; the caller's has its own variables.
  await press(page, "F11");
  await page.waitFor(`${where}.startsWith('e2e_dbg_double')`, 20_000);
  await page.waitFor(`${tab}.querySelectorAll('.debug-frame').length === 2`, 10_000);
  await page.waitFor(`${variable("x")} === '1'`, 10_000);
  await page.eval(`${tab}.querySelector('.debug-frame[data-level="1"]').click()`);
  await page.waitFor(`${variable("total")} === '0' && ${where}.startsWith('e2e_dbg_sum')`, 10_000);
  await page.eval(`${tab}.querySelector('.debug-frame[data-level="0"]').click()`);
  await page.waitFor(`${variable("x")} === '1'`, 10_000);

  // F10 out of it: the next statement of the loop, for the next value.
  await press(page, "F10");
  await page.waitFor(`${where}.startsWith('e2e_dbg_sum') && ${where}.endsWith('line ${INSERT_LINE}')`, 20_000);
  await page.waitFor(`${variable("i")} === '2' && ${variable("total")} === '2'`, 10_000);

  // A changed variable carries on; a value of the wrong type is refused.
  const edit = async (value) => {
    await page.eval(`${tab}.querySelector('[data-field="debug-variables"] tr[data-name="total"] .debug-value-text').click()`);
    await page.waitFor(`${inTab('[data-field="debug-variable-input"]')} !== null`);
    await setValue(page, inTab('[data-field="debug-variable-input"]'), value);
    await page.eval(`${inTab('[data-field="debug-variable-input"]')}.focus()`);
    await page.key("Enter", "Enter", 13);
    await page.waitFor(`${inTab('[data-field="debug-variable-input"]')} === null`);
  };
  await edit("100");
  await page.waitFor(`${variable("total")} === '100'`, 10_000);
  await edit("abc");
  await page.waitFor(`${tabError}.includes('total')`, 10_000);
  assert.equal(await page.eval(variable("total")), "100");

  // The breakpoint cleared, F5 runs to the end: 100 + 4 + 6.
  await clickGutter(page, BODY_LINE);
  await page.waitFor(`${breakpointLines} === '[]'`);
  await press(page, "F5");
  await page.waitFor(`${statusOf} === 'done'`, 30_000);
  assert.equal(await page.eval(`${inTab('[data-field="debug-rows"] tbody td')}.textContent`), "110");
  assert.ok((await page.eval(`${inTab('[data-field="debug-messages"]')}.textContent`)).includes("total 110"));
  for (const field of ["debug-into", "debug-over", "debug-continue", "debug-stop"]) {
    assert.equal(await page.eval(`${inTab(`[data-field="${field}"]`)}.disabled`), true, field);
  }
  assert.equal(sh(DB, "SELECT count(*) FROM e2e_dbg_log;"), "3", "what the call wrote is committed");
  await noCallLeft(page);
  await closeTab(page);
}

async function closeTab(page) {
  await page.eval(`document.querySelector('.tab.active button[aria-label="Close tab"]').click()`);
  await page.waitFor(`${tab} === undefined`);
}

/** Stop while paused, Stop while a continue runs, and a closed tab: nothing stays running,
 *  and what an aborted call wrote is rolled back. */
async function stopping(page) {
  await debugSum(page, "1000");
  await press(page, "F10");
  await page.waitFor(`${where}.endsWith('line ${INSERT_LINE}')`, 20_000);
  await press(page, "F10");
  await page.waitFor(`${where}.endsWith('line ${BODY_LINE}')`, 20_000);
  await press(page, "F5", SHIFT);
  await page.waitFor(`${statusOf} === 'aborted'`, 30_000);
  assert.ok(await page.eval(`${inTab('[data-field="debug-result"]')} !== null`));
  assert.equal(sh(DB, "SELECT count(*) FROM e2e_dbg_log;"), "3", "the aborted call's row is rolled back");
  await noCallLeft(page);
  await closeTab(page);

  await openDialog(page, SPIN);
  assert.equal(await page.eval(`${dialog}.querySelector('[data-field="debug-arguments"]').children.length`), 0);
  await page.eval(`${dialog}.querySelector('[data-field="debug-start"]').click()`);
  await page.waitFor(`${dialog} === null && ${tab} !== undefined`, 30_000);
  await page.waitFor(`${statusOf} === 'stopped'`, 30_000);
  await page.eval(`${inTab('[data-field="debug-continue"]')}.click()`);
  await page.waitFor(`${statusOf} === 'running'`, 10_000);
  await page.sleep(500);
  assert.equal(await page.eval(statusOf), "running", "the loop runs on");
  assert.equal(await page.eval(`${inTab('[data-field="debug-stop"]')}.disabled`), false);
  await page.eval(`${inTab('[data-field="debug-stop"]')}.click()`);
  await page.waitFor(`${statusOf} === 'aborted'`, 30_000);
  await noCallLeft(page);
  await closeTab(page);

  // Closing the tab while paused aborts the call too.
  await debugSum(page, "1000");
  await closeTab(page);
  await noCallLeft(page);
  assert.equal(sh(DB, "SELECT count(*) FROM e2e_dbg_log;"), "3");
}

export default async function (page, { shots }) {
  try {
    await setupChecks(page);
    await debugToTheEnd(page);
    if (shots) await page.screenshot(`${shots}/admin-debugger-postgres.png`);
    await stopping(page);
  } finally {
    sh(
      DB,
      `DROP FUNCTION IF EXISTS e2e_dbg_sum(integer, integer), e2e_dbg_double(integer), e2e_dbg_spin(),
         e2e_dbg_sql(integer), e2e_dbg_trg();
       DROP TABLE IF EXISTS e2e_dbg_log;`,
    );
  }
}
