// Group J against the dev PostgreSQL from docker/compose.yml: Server activity lists the
// sessions, shows one waiting for another's lock (in the list, in the waiting chains and in
// the locks), cancels the waiting statement, ends the blocking session, and does not let
// the app end its own. Skipped when the server is not running.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { adminButton, adminTab, confirmDanger, showSection } from "../admin.mjs";

export { setup, skip } from "./dev-postgres.mjs";

const NAME = "E2E PG";
const TABLE = "demo.e2e_admin_lock";
const row = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;
/** The row of the session whose application is `app`. */
const sessionRow = (app) =>
  `[...(${adminTab}?.querySelectorAll('tr.admin-session') ?? [])].find(r => r.children[4]?.textContent === ${JSON.stringify(app)})`;

function psql(sql) {
  const r = spawnSync("docker", ["exec", "-i", "platypus-db-postgres-1", "psql", "-v", "ON_ERROR_STOP=1", "-q", "-U", "platypus", "-d", "platypus"], {
    input: sql,
    encoding: "utf8",
  });
  if (r.status !== 0) throw new Error(`psql: ${r.stderr}`);
}

/** A psql session running `sql` in the background, named `app`. */
function background(app, sql) {
  const child = spawn("docker", ["exec", "-i", "platypus-db-postgres-1", "psql", "-q", "-U", "platypus", "-d", "platypus"], {
    stdio: ["pipe", "ignore", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", (d) => (stderr += d));
  const exited = new Promise((resolve) => child.on("exit", () => resolve(stderr)));
  child.stdin.end(`SET application_name = '${app}';\n${sql}\n`);
  return { child, exited };
}

export default async function (page) {
  psql(`DROP TABLE IF EXISTS ${TABLE}; CREATE TABLE ${TABLE} (id int PRIMARY KEY, v int); INSERT INTO ${TABLE} VALUES (1, 0);`);
  const holder = background("e2e-holder", `BEGIN; UPDATE ${TABLE} SET v = 1 WHERE id = 1; SELECT pg_sleep(300);`);
  const waiter = background("e2e-waiter", `UPDATE ${TABLE} SET v = 2 WHERE id = 1;`);
  try {
    await page.waitFor(`${row} !== undefined`);
    await page.eval(`${row}.querySelector(".twisty").click()`);
    await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
    await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
    await page.type("platypus");
    await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
    await page.waitFor(`document.querySelector('.modal') === null`, 15_000);

    await page.eval(`${row}.id = 'e2e-connection'`);
    const r = await page.rect("#e2e-connection");
    await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
    await page.waitFor(`document.querySelector('.context-menu') !== null`);
    await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Server activity')).click()`);
    await page.waitFor(`${adminTab} !== undefined`, 15_000);

    // The waiter is listed, marked, and names the holder as the session it waits for.
    await page.waitFor(`${sessionRow("e2e-waiter")}?.classList.contains('blocked') ?? false`, 20_000);
    const holderId = await page.eval(`${sessionRow("e2e-holder")}.children[0].textContent.trim()`);
    const waiterId = await page.eval(`${sessionRow("e2e-waiter")}.children[0].textContent.trim()`);
    assert.equal(await page.eval(`${sessionRow("e2e-waiter")}.children[8].textContent`), holderId);
    assert.match(await page.eval(`${sessionRow("e2e-waiter")}.children[9].textContent`), /UPDATE demo\.e2e_admin_lock SET v = 2/);
    // The app's own session is first and marked.
    assert.match(await page.eval(`${adminTab}.querySelector('tr.admin-session').textContent`), /this app/);
    assert.equal(await page.eval(`${adminTab}.querySelectorAll('tr.admin-session.current').length`), 1);
    // The Locks section counts the session that waits.
    assert.equal(await page.eval(`${adminTab}.querySelector('.admin-blocked-count')?.textContent`), "1");

    // The filter keeps the matching sessions only; with nothing matching it says so.
    await page.eval(`${adminTab}.querySelector('input[type=search]').focus()`);
    await page.type("e2e-");
    await page.waitFor(`${adminTab}.querySelectorAll('tr.admin-session').length === 2`);
    await page.type("nothing-like-this");
    await page.waitFor(`${adminTab}.querySelector('.admin-scroll p')?.textContent === 'No sessions match.'`);
    await page.eval(`(() => {
      const input = ${adminTab}.querySelector('input[type=search]');
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, '');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    })()`);
    await page.waitFor(`${adminTab}.querySelectorAll('tr.admin-session').length > 2`);

    // Locks: the chain from the holder to the waiter, and the lock waited for.
    await showSection(page, "Locks");
    await page.waitFor(`${adminTab}.querySelector('.admin-chains') !== null`, 10_000);
    assert.equal(await page.eval(`${adminTab}.querySelector('.admin-chains > li > button .admin-blocker').textContent`), holderId);
    assert.equal(await page.eval(`${adminTab}.querySelector('.admin-chains > li > ul .admin-waiter').textContent`), waiterId);
    await page.waitFor(`${adminTab}.querySelector('table.admin-table tr.blocked') !== null`, 10_000);
    await page.eval(`${adminTab}.querySelector('.admin-toolbar input[type=checkbox]').click()`);
    const statuses = await page.eval(`[...${adminTab}.querySelectorAll('table.admin-table tbody tr')].map(r => r.children[4].textContent)`);
    assert.ok(statuses.length > 0 && statuses.every((s) => s === "Waiting"), `only waiting locks: ${statuses}`);
    assert.ok(
      await page.eval(`[...${adminTab}.querySelectorAll('table.admin-table tbody tr')].some(r => r.children[0].textContent === ${JSON.stringify(waiterId)})`),
    );
    // A session in a chain opens in the session list, selected.
    await page.eval(`${adminTab}.querySelector('.admin-chains .admin-waiter').closest('button').click()`);
    await page.waitFor(`${adminTab}.querySelector('.admin-details strong')?.textContent === 'Session ${waiterId}'`);

    // Cancelling the waiting statement ends it with PostgreSQL's cancel error.
    assert.match(await page.eval(`${adminTab}.querySelector('.admin-statement').textContent`), /SET v = 2/);
    await page.eval(`${adminButton("Cancel statement")}.click()`);
    await confirmDanger(page);
    assert.match(await waiter.exited, /canceling statement due to user request/);
    await page.waitFor(`${sessionRow("e2e-waiter")} === undefined`, 20_000);

    // Asking to end a session and cancelling the question leaves it.
    await page.eval(`${sessionRow("e2e-holder")}.click()`);
    await page.eval(`${adminButton("End session")}.click()`);
    await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
    assert.match(await page.eval(`document.querySelector('.modal.confirm p').textContent`), new RegExp(`End session ${holderId} of platypus`));
    await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Cancel').click()`);
    await page.waitFor(`document.querySelector('.modal.confirm') === null`);
    assert.notEqual(await page.eval(`${sessionRow("e2e-holder")}`), undefined);

    // Ending it closes the holder's psql and rolls back its update.
    await page.eval(`${adminButton("End session")}.click()`);
    await confirmDanger(page);
    assert.match(await holder.exited, /terminat/);
    await page.waitFor(`${sessionRow("e2e-holder")} === undefined`, 20_000);

    // The app's own session cannot be ended, nor its statement cancelled.
    await page.eval(`${adminTab}.querySelector('tr.admin-session.current').click()`);
    assert.equal(await page.eval(`${adminButton("End session")}.disabled`), true);
    assert.equal(await page.eval(`${adminButton("Cancel statement")}.disabled`), true);

    // Manual refresh: the list stays as it was until asked.
    await page.eval(`(() => {
      const s = ${adminTab}.querySelector('.admin-interval');
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, '0');
      s.dispatchEvent(new Event('change', { bubbles: true }));
    })()`);
    const late = background("e2e-late", "SELECT pg_sleep(60);");
    try {
      await new Promise((resolve) => setTimeout(resolve, 3000));
      assert.equal(await page.eval(`${sessionRow("e2e-late")}`), undefined, "a manual list refreshed by itself");
      await page.eval(`${adminTab}.querySelector('button[aria-label="Refresh now"]').click()`);
      await page.waitFor(`${sessionRow("e2e-late")} !== undefined`, 10_000);
    } finally {
      late.child.kill();
    }
  } finally {
    holder.child.kill();
    waiter.child.kill();
    psql(`SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name LIKE 'e2e-%'; DROP TABLE IF EXISTS ${TABLE};`);
  }
}
