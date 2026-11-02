// Group J against the dev PostgreSQL: Server activity lists the sessions with their badges
// and counts, filters them by activity and by text, shows one waiting for another's lock
// (in the list, in the details, in the waiting chains and in the locks), cancels the
// waiting statement, ends the blocking session, does not let the app end its own, and
// refreshes on demand when set to Manual.
import { assert, connections, needs, sh } from "../qa-grid.mjs";
import {
  adminButton,
  adminTab,
  background,
  checkChain,
  confirmDanger,
  field,
  idOf,
  openAdmin,
  rowWith,
  select,
  showSection,
  waitBlocked,
} from "../admin.mjs";

const DB = "pg";
const NAME = "QA J PostgreSQL";
const TABLE = "demo.e2e_admin_lock";

export const skip = needs(DB);

export function setup({ dir }) {
  connections(dir, [[NAME, DB]]);
}

const filterCount = (activity) => `${adminTab}.querySelector('.admin-filter.${activity} strong')?.textContent`;

async function setSearch(page, text) {
  await page.eval(`(() => {
    const input = ${adminTab}.querySelector('input[type=search]');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, ${JSON.stringify(text)});
    input.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
}

export default async function (page) {
  sh(DB, `DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, v int); INSERT INTO ${TABLE} VALUES (1, 0);`);
  const holder = background(DB, `SET application_name = 'e2e-holder';\nBEGIN; UPDATE ${TABLE} SET v = 1 WHERE id = 1; SELECT pg_sleep(300) AS e2e_holder;\n`);
  const waiter = background(DB, `SET application_name = 'e2e-waiter';\nUPDATE ${TABLE} SET v = 2 WHERE id = 1;\n`);
  try {
    await openAdmin(page, NAME, DB);

    // The waiter is marked and its details name the holder and the lock it waits for.
    await page.waitFor(`${rowWith("e2e_holder")} !== undefined`, 30_000);
    const holderId = await idOf(page, "e2e_holder");
    await waitBlocked(page, "SET v = 2", holderId);
    const waiterId = await idOf(page, "SET v = 2");
    assert.match(await page.eval(field("wait")), /^Lock:/);
    assert.equal(await page.eval(field("application")), "e2e-waiter");
    assert.match(await page.eval(`${adminTab}.querySelector('.admin-statement').textContent`), /SET v = 2/);
    assert.equal(await page.eval(`${rowWith("SET v = 2")}.querySelector('.admin-badge').textContent`), "waiting");
    assert.equal(await page.eval(`${rowWith("e2e_holder")}.querySelector('.admin-badge').textContent`), "running");

    // The app's own session is first and marked; the Locks section counts the waiter.
    assert.match(await page.eval(`${adminTab}.querySelector('tr.admin-session').textContent`), /this app/);
    assert.equal(await page.eval(`${adminTab}.querySelectorAll('tr.admin-session.current').length`), 1);
    assert.equal(await page.eval(`${adminTab}.querySelector('.admin-blocked-count')?.textContent`), "1");
    assert.equal(await page.eval(filterCount("waiting")), "1");

    // The counts filter by activity: the waiting one only, then all again.
    await page.eval(`${adminTab}.querySelector('.admin-filter.waiting').click()`);
    await page.waitFor(`${adminTab}.querySelectorAll('tr.admin-session').length === 1`);
    assert.equal(await page.eval(`${adminTab}.querySelector('tr.admin-session').dataset.id`), waiterId);
    await page.eval(`${adminTab}.querySelector('.admin-filter').click()`);
    await page.waitFor(`${adminTab}.querySelectorAll('tr.admin-session').length > 2`);

    // The text filter keeps the matching sessions; with nothing matching it says so.
    await setSearch(page, "e2e-");
    await page.waitFor(`${adminTab}.querySelectorAll('tr.admin-session').length === 2`);
    await setSearch(page, "nothing-like-this");
    await page.waitFor(`${adminTab}.querySelector('.admin-empty')?.textContent === 'No sessions match.'`);
    await setSearch(page, "");
    await page.waitFor(`${adminTab}.querySelectorAll('tr.admin-session').length > 2`);

    // Locks: the chain from the holder to the waiter, and only the waited lock when asked.
    await checkChain(page, holderId, waiterId);
    await showSection(page, "Locks");
    await page.waitFor(`${adminTab}.querySelector('table.admin-locks') !== null`, 10_000);
    await page.eval(`${adminTab}.querySelector('.admin-toolbar input[type=checkbox]').click()`);
    const statuses = await page.eval(`[...${adminTab}.querySelectorAll('table.admin-locks tbody tr')].map(r => r.children[4].textContent)`);
    assert.ok(statuses.length > 0 && statuses.every((s) => s === "Waiting"), `only waiting locks: ${statuses}`);
    // A session in a chain opens in the session list, selected.
    await page.eval(`${adminTab}.querySelector('.admin-chains > li > ul button').click()`);
    await page.waitFor(`${adminTab}.querySelector('.admin-details-title strong')?.textContent === 'Session ${waiterId}'`);

    // Cancelling the waiting statement ends it with PostgreSQL's cancel error.
    await page.eval(`${adminButton("Cancel statement")}.click()`);
    await confirmDanger(page);
    assert.match(await waiter.exited, /canceling statement due to user request/);
    await page.waitFor(`${rowWith("SET v = 2")} === undefined`, 20_000);

    // Asking to end a session and cancelling the question leaves it.
    await select(page, "e2e_holder");
    await page.eval(`${adminButton("End session")}.click()`);
    await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
    assert.match(await page.eval(`document.querySelector('.modal.confirm p').textContent`), new RegExp(`End session ${holderId} of platypus`));
    await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Cancel').click()`);
    await page.waitFor(`document.querySelector('.modal.confirm') === null`);
    assert.equal(await page.eval(`${rowWith("e2e_holder")} !== undefined`), true);

    // Ending it closes the holder's psql and rolls back its update.
    await page.eval(`${adminButton("End session")}.click()`);
    await confirmDanger(page);
    await page.waitFor(`[...document.querySelectorAll('.toaster .toast')].some(t => t.textContent === 'Session ${holderId} ended')`, 20_000);
    assert.match(await holder.exited, /terminat/);
    await page.waitFor(`${rowWith("e2e_holder")} === undefined`, 20_000);
    assert.equal(sh(DB, `SELECT v FROM ${TABLE} WHERE id = 1`), "0", "the holder's update was rolled back");

    // The app's own session cannot be ended, nor its statement cancelled.
    await page.eval(`${adminTab}.querySelector('tr.admin-session.current').click()`);
    await page.waitFor(`${adminTab}.querySelector('.admin-details-title') !== null`);
    assert.equal(await page.eval(`${adminButton("End session")}.disabled`), true);
    assert.equal(await page.eval(`${adminButton("Cancel statement")}.disabled`), true);

    // Manual refresh: the list stays as it was until asked.
    await page.eval(`(() => {
      const s = ${adminTab}.querySelector('.admin-interval');
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, '0');
      s.dispatchEvent(new Event('change', { bubbles: true }));
    })()`);
    const late = background(DB, "SELECT pg_sleep(60) AS e2e_late;\n");
    try {
      await page.sleep(3000);
      assert.equal(await page.eval(`${rowWith("e2e_late")} === undefined`), true, "a manual list refreshed by itself");
      await page.eval(`${adminTab}.querySelector('button[aria-label="Refresh now"]').click()`);
      await page.waitFor(`${rowWith("e2e_late")} !== undefined`, 10_000);
    } finally {
      late.child.kill();
    }
  } finally {
    for (const s of [holder, waiter]) s.child.kill();
    sh(DB, `SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE pid <> pg_backend_pid() AND (application_name LIKE 'e2e-%' OR query LIKE '%e2e_late%'); DROP TABLE IF EXISTS ${TABLE};`);
  }
}
