// Group J against the dev MySQL (and MariaDB, admin-sessions-mariadb.mjs): Server activity
// shows a session waiting for another's row lock, with the chain and the waited lock;
// KILL QUERY cancels the waiting statement, KILL ends the holder and rolls back its update.
import { assert, connections, needs, sh } from "../qa-grid.mjs";
import { adminButton, background, checkChain, confirmDanger, idOf, openAdmin, rowWith, select, waitBlocked } from "../admin.mjs";

const DB = "mysql";
const NAME = "QA J MySQL";

export const skip = needs(DB);

export function setup({ dir }) {
  connections(dir, [[NAME, DB]]);
}

export async function lockFlow(page, db, name) {
  sh(db, "DROP TABLE IF EXISTS e2e_admin_lock; CREATE TABLE e2e_admin_lock (id INT PRIMARY KEY, v INT) ENGINE=InnoDB; INSERT INTO e2e_admin_lock VALUES (1, 0);");
  const holder = background(db, "BEGIN; UPDATE e2e_admin_lock SET v = 1 WHERE id = 1; SELECT SLEEP(300) AS e2e_holder;\n");
  // The waiter starts once the holder sleeps with its row locked: two docker processes started
  // together may run in either order, and a waiter first would simply win the row.
  const sleeping = "SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE INFO LIKE '%SLEEP(300) AS e2e_holder%' AND INFO NOT LIKE '%PROCESSLIST%'";
  for (let i = 0; i < 100 && sh(db, sleeping) === "0"; i++) await new Promise((r) => setTimeout(r, 200));
  const waiter = background(db, "UPDATE e2e_admin_lock SET v = 2 WHERE id = 1;\n");
  try {
    await openAdmin(page, name, db);
    await page.waitFor(`${rowWith("e2e_holder")} !== undefined`, 30_000);
    const holderId = await idOf(page, "e2e_holder");
    await waitBlocked(page, "SET v = 2", holderId);
    const waiterId = await idOf(page, "SET v = 2");
    await checkChain(page, holderId, waiterId);

    // KILL QUERY: the waiting update fails as interrupted.
    await select(page, "SET v = 2");
    await page.eval(`${adminButton("Cancel statement")}.click()`);
    await confirmDanger(page);
    assert.match(await waiter.exited, /interrupted/);
    await page.waitFor(`${rowWith("SET v = 2")} === undefined`, 30_000);

    // KILL: the holder's session goes, and its update with it.
    await select(page, "e2e_holder");
    await page.eval(`${adminButton("End session")}.click()`);
    await confirmDanger(page);
    await page.waitFor(`${rowWith("e2e_holder")} === undefined`, 30_000);
    assert.match(await holder.exited, /ERROR/);
    assert.equal(sh(db, "SELECT v FROM e2e_admin_lock WHERE id = 1"), "0", "the holder's update was rolled back");
  } finally {
    holder.child.kill();
    waiter.child.kill();
    sh(db, "DROP TABLE IF EXISTS e2e_admin_lock");
  }
}

export default async function (page) {
  await lockFlow(page, DB, NAME);
}
