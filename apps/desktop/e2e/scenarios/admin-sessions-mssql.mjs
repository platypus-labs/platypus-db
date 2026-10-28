// Group J against the dev SQL Server: Server activity shows a session waiting for another's
// key lock, with the chain and the waited lock. SQL Server cannot cancel another session's
// statement, so there is no Cancel statement; KILL ends the waiter, then the holder.
import { assert, connections, needs, sh } from "../qa-grid.mjs";
import { adminButton, background, checkChain, confirmDanger, field, idOf, openAdmin, rowOf, rowWith, select, waitBlocked } from "../admin.mjs";

const DB = "mssql";
const NAME = "QA J SQL Server";

export const skip = needs(DB);

export function setup({ dir }) {
  sh(DB, "IF OBJECT_ID('dbo.e2e_admin_lock') IS NOT NULL DROP TABLE dbo.e2e_admin_lock; CREATE TABLE dbo.e2e_admin_lock (id INT PRIMARY KEY, v INT); INSERT INTO dbo.e2e_admin_lock VALUES (1, 0);");
  connections(dir, [[NAME, DB]]);
}

export default async function (page) {
  const holder = background(DB, "BEGIN TRAN; UPDATE dbo.e2e_admin_lock SET v = 1 WHERE id = 1; WAITFOR DELAY '00:05:00' /* e2e_holder */;\nGO\n");
  const waiter = background(DB, "UPDATE dbo.e2e_admin_lock SET v = 2 WHERE id = 1;\nGO\n");
  try {
    await openAdmin(page, NAME, DB);
    await page.waitFor(`${rowWith("e2e_holder")} !== undefined`, 30_000);
    const holderId = await idOf(page, "e2e_holder");
    await waitBlocked(page, "SET v = 2", holderId);
    const waiterId = await idOf(page, "SET v = 2");
    // The wait type of a waiting key lock.
    assert.match(await page.eval(field("wait")), /^LCK_M_/);
    await checkChain(page, holderId, waiterId);

    await select(page, "SET v = 2");
    assert.equal(await page.eval(`${adminButton("Cancel statement")}`), undefined, "no Cancel statement on SQL Server");
    await page.eval(`${adminButton("End session")}.click()`);
    await confirmDanger(page);
    await page.waitFor(`${rowOf(waiterId)} === null`, 30_000);

    await select(page, "e2e_holder");
    await page.eval(`${adminButton("End session")}.click()`);
    await confirmDanger(page);
    await page.waitFor(`${rowWith("e2e_holder")} === undefined`, 30_000);
    assert.equal(sh(DB, "SET NOCOUNT ON; SELECT v FROM dbo.e2e_admin_lock WHERE id = 1"), "0", "the holder's update was rolled back");
  } finally {
    holder.child.kill();
    waiter.child.kill();
    sh(DB, "IF OBJECT_ID('dbo.e2e_admin_lock') IS NOT NULL DROP TABLE dbo.e2e_admin_lock");
  }
}
